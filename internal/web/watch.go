package web

import (
	"crypto/sha1"
	"encoding/hex"
	"os"
	"strconv"
	"sync"
	"time"
)

// watchEvery is how often the watched files are hashed. A stat would miss a
// change within one timestamp tick; hashing a few notes every two seconds is
// cheap.
const watchEvery = 2 * time.Second

// watchBehind is how many messages a slow reader may fall behind before the
// next is dropped for it: the latest fingerprint is all it needs, and a stuck
// reader must not hold the watcher.
const watchBehind = 8

// blobID is the id git gives a file's content: the fingerprint an edit form
// carries, so it also names the version the edit began from.
func blobID(path string) (string, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	hash := sha1.New()
	hash.Write([]byte("blob " + strconv.Itoa(len(content)) + "\x00"))
	hash.Write(content)
	return hex.EncodeToString(hash.Sum(nil)), nil
}

type subscriber struct {
	changes chan string
	gone    <-chan struct{}
}

type watched struct {
	path        string
	seen        string
	subscribers []subscriber
}

type watchKey struct{ book, id string }

// Watch tells readers when a note's file changes, so an open editor can warn
// before a save would collide. It sends the fingerprint, never the content,
// which is somebody's prose.
type Watch struct {
	mutex    sync.Mutex
	notes    map[watchKey]*watched
	stopping bool
	stop     chan struct{}
	finished chan struct{}
}

// NewWatch starts the goroutine that looks; Stop ends it.
func NewWatch() *Watch {
	watch := &Watch{
		notes:    map[watchKey]*watched{},
		stop:     make(chan struct{}),
		finished: make(chan struct{}),
	}
	go watch.look()
	return watch
}

// Subscribe starts telling a reader about a note. now is the fingerprint it
// already has. The channel is closed by Stop, or once gone is
// closed (a request's context) and the watcher has noticed.
func (w *Watch) Subscribe(book, id, path, now string, gone <-chan struct{}) <-chan string {
	changes := make(chan string, watchBehind)
	w.mutex.Lock()
	defer w.mutex.Unlock()
	if w.stopping {
		close(changes)
		return changes
	}
	key := watchKey{book, id}
	entry, ok := w.notes[key]
	if !ok {
		entry = &watched{path: path, seen: now}
		w.notes[key] = entry
	}
	entry.subscribers = append(entry.subscribers, subscriber{changes: changes, gone: gone})
	return changes
}

// Stop ends every stream, which the server must do before it waits for
// requests: a watch never finishes by itself.
func (w *Watch) Stop() {
	w.mutex.Lock()
	if w.stopping {
		w.mutex.Unlock()
		return
	}
	w.stopping = true
	for _, entry := range w.notes {
		for _, each := range entry.subscribers {
			close(each.changes)
		}
	}
	w.notes = map[watchKey]*watched{}
	w.mutex.Unlock()
	close(w.stop)
	<-w.finished
}

func (w *Watch) look() {
	defer close(w.finished)
	ticker := time.NewTicker(watchEvery)
	defer ticker.Stop()
	for {
		select {
		case <-w.stop:
			return
		case <-ticker.C:
			w.tick()
		}
	}
}

// tick hashes outside the lock, so subscribing never waits on the disk.
func (w *Watch) tick() {
	for key, path := range w.wanted() {
		hash, err := blobID(path)
		// A renamed or deleted note is skipped, not reported as changed.
		if err != nil {
			continue
		}
		w.tell(key, hash)
	}
}

func (w *Watch) wanted() map[watchKey]string {
	w.mutex.Lock()
	defer w.mutex.Unlock()
	wanted := map[watchKey]string{}
	for key, entry := range w.notes {
		entry.subscribers = stillListening(entry.subscribers)
		if len(entry.subscribers) == 0 {
			delete(w.notes, key)
			continue
		}
		wanted[key] = entry.path
	}
	return wanted
}

func stillListening(subscribers []subscriber) []subscriber {
	kept := subscribers[:0]
	for _, each := range subscribers {
		select {
		case <-each.gone:
			// Under the watch's lock, as is every send and Stop's close, so
			// it is closed once.
			close(each.changes)
		default:
			kept = append(kept, each)
		}
	}
	return kept
}

func (w *Watch) tell(key watchKey, hash string) {
	w.mutex.Lock()
	defer w.mutex.Unlock()
	entry, ok := w.notes[key]
	if !ok || entry.seen == hash {
		return
	}
	entry.seen = hash
	for _, each := range entry.subscribers {
		// Never wait on a reader while holding the lock.
		select {
		case each.changes <- hash:
		default:
		}
	}
}
