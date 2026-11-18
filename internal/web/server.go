package web

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
)

// Options is what Serve needs.
type Options struct {
	Env        cmd.Env
	Listen     string
	AllowHosts []string
	LogFormat  LogFormat
	LogLevel   slog.Level
	// Stdout gets the startup and shutdown lines; Stderr gets the log.
	Stdout io.Writer
	Stderr io.Writer
}

// Server is the notebooks over HTTP, for reading and writing them from a
// phone. It reads through notebook and writes through cmd; a notebook is named
// in the URL, never taken from the active pointer, which belongs to a shell.
type Server struct {
	env      cmd.Env
	guard    Guard
	logger   *slog.Logger
	writing  locks
	errands  *Errands
	watching *Watch
}

// NewServer is the handler's state. Stop its Watch by Shutdown.
func NewServer(opts Options) *Server {
	return &Server{
		env:      opts.Env,
		guard:    NewGuard(opts.AllowHosts),
		logger:   NewLogger(opts.Stderr, opts.LogLevel, opts.LogFormat),
		errands:  NewErrands(),
		watching: NewWatch(),
	}
}

// readHeaderTimeout keeps a client that sends its headers a byte at a time from
// holding a connection open for good.
const readHeaderTimeout = 15 * time.Second

// newHTTPServer sets no WriteTimeout: it would cut the watch stream.
func newHTTPServer(handler http.Handler, silent *silentConns) *http.Server {
	return &http.Server{Handler: handler, ConnState: silent.track, ReadHeaderTimeout: readHeaderTimeout}
}

// Serve runs until it is asked to stop. Unlike every other command it prints
// the address rather than returning it, since it does not finish while in use.
//
// Stopping, in order: a signal closes the listener, in-flight requests are
// answered (finishing any commit), then errands are waited for. Only then does
// it return nil, so a supervisor can tell a clean stop from a crash.
func Serve(opts Options) error {
	// Before the bind, so a failure to listen is logged.
	server := NewServer(opts)
	// First, so an early signal has somewhere to go.
	signals := make(chan os.Signal, 2)
	signal.Notify(signals, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(signals)

	listener, err := net.Listen("tcp", opts.Listen)
	if err != nil {
		server.watching.Stop()
		return fmt.Errorf("could not listen on %s: %w", opts.Listen, err)
	}
	announce(opts.Stdout, listener.Addr())

	silent := &silentConns{}
	httpServer := newHTTPServer(server.Handler(), silent)
	served := make(chan error, 1)
	go func() { served <- httpServer.Serve(listener) }()

	select {
	case err := <-served:
		server.watching.Stop()
		return err
	case first := <-signals:
		fmt.Fprintf(opts.Stdout, "%s — finishing what is in flight\n", signalName(first))
		// Before the wait: a watch never finishes by itself, so shutdown would
		// wait on it forever.
		server.watching.Stop()
		silent.closeAll()
		go server.abandonOnSecond(signals, opts.Stdout)
	}
	// The second signal cuts short the errands, which are unbounded (a push to
	// a dead host is minutes of git on a socket), not the requests in flight.
	if err := httpServer.Shutdown(context.Background()); err != nil {
		return err
	}
	return server.settle(opts.Stdout)
}

func (s *Server) abandonOnSecond(signals <-chan os.Signal, out io.Writer) {
	second := <-signals
	fmt.Fprintf(out, "%s again — not waiting\n", signalName(second))
	s.errands.Abandon()
}

// settle waits for the errands; signalled twice, one was cut off, so that is a
// failure, not a clean stop.
func (s *Server) settle(out io.Writer) error {
	for _, left := range s.errands.Running() {
		fmt.Fprintf(out, "waiting for %s in %s — signal again to leave it unfinished\n", left.Job.Name(), left.Book)
	}
	left := s.errands.Settle()
	if len(left) == 0 {
		return nil
	}
	names := make([]string, len(left))
	for i, each := range left {
		names[i] = each.Job.Name() + " in " + each.Book
	}
	return fmt.Errorf("left %s unfinished", strings.Join(names, ", "))
}

func announce(out io.Writer, address net.Addr) {
	fmt.Fprintf(out, "noda is at http://%s\n", address)
	if tcp, ok := address.(*net.TCPAddr); ok {
		if ip, ok := netip.AddrFromSlice(tcp.IP); ok && !ip.Unmap().IsLoopback() {
			fmt.Fprintln(out, "reachable from the network — there is no password on it")
		}
	}
}

// signalName names it when printed, so a container log shows the orchestrator
// stopped it rather than that it fell over.
func signalName(received os.Signal) string {
	if received == syscall.SIGTERM {
		return "SIGTERM"
	}
	return "SIGINT"
}

// workContext is the context a handler's git runs use. A write must finish
// once begun (a killed git leaves a file written but not committed), so it
// ignores the client hanging up; a read has no use for its answer then.
func workContext(r *http.Request) context.Context {
	switch r.Method {
	case http.MethodGet, http.MethodHead:
		return r.Context()
	}
	return context.WithoutCancel(r.Context())
}

func (s *Server) openNotebook(ctx context.Context, book string) (*notebook.Notebook, bool, error) {
	if !notebook.Exists(s.env.Paths, book) {
		return nil, false, nil
	}
	opened, err := notebook.Open(s.env.Git, s.env.Paths, book)
	if err != nil {
		return nil, false, err
	}
	return opened.WithContext(ctx), true, nil
}
