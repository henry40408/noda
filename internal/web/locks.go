package web

import "sync"

// locks is one write lock per notebook: a single lock froze Save on every
// notebook whenever one notebook's remote went quiet. It does not guard against
// a terminal writing at the same time; the fingerprint does.
type locks struct {
	mutex sync.Mutex
	each  map[string]*sync.Mutex
}

// of is a pointer, so the map is free again at once and one notebook's slow
// push does not block finding another's lock.
func (l *locks) of(book string) *sync.Mutex {
	l.mutex.Lock()
	defer l.mutex.Unlock()
	if l.each == nil {
		l.each = map[string]*sync.Mutex{}
	}
	lock, ok := l.each[book]
	if !ok {
		lock = &sync.Mutex{}
		l.each[book] = lock
	}
	return lock
}
