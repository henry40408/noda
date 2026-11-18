package web

import (
	"net"
	"net/http"
	"sync"
)

// silentConns remembers connections that have sent nothing. Shutdown waits
// five seconds on those before it counts them idle; a tab left open must not
// hold a stop up, so they are closed at once.
type silentConns struct {
	mutex sync.Mutex
	conns map[net.Conn]struct{}
}

func (s *silentConns) track(conn net.Conn, state http.ConnState) {
	s.mutex.Lock()
	defer s.mutex.Unlock()
	if state == http.StateNew {
		if s.conns == nil {
			s.conns = map[net.Conn]struct{}{}
		}
		s.conns[conn] = struct{}{}
		return
	}
	delete(s.conns, conn)
}

func (s *silentConns) closeAll() {
	s.mutex.Lock()
	defer s.mutex.Unlock()
	for conn := range s.conns {
		_ = conn.Close()
	}
}
