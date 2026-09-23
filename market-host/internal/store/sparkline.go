// Package store keeps per-symbol server-side session state so that
// reconnecting clients receive a full sparkline without history plumbing.
package store

import "sync"

// Sparkline is a fixed-capacity ring buffer of last prices (阶段一: 30~60 点).
type Sparkline struct {
	mu   sync.RWMutex
	buf  []float64
	cap  int
	full bool
}

// NewSparkline caps at n points (contract: 60).
func NewSparkline(n int) *Sparkline {
	if n < 1 {
		n = 1
	}
	return &Sparkline{buf: make([]float64, 0, n), cap: n}
}

// Push appends a price; duplicates only advance when the value moves
// (identical consecutive prices collapse to one point).
func (s *Sparkline) Push(v float64) {
	if v <= 0 {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if n := len(s.buf); n > 0 && s.buf[n-1] == v {
		return
	}
	if len(s.buf) == s.cap {
		s.buf = s.buf[1:]
		s.full = true
	}
	s.buf = append(s.buf, v)
}

// Snapshot returns a copy of the buffered points (never the live slice).
func (s *Sparkline) Snapshot() []float64 {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]float64, len(s.buf))
	copy(out, s.buf)
	if len(out) == 0 {
		return nil
	}
	return out
}

// Len reports buffered point count.
func (s *Sparkline) Len() int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.buf)
}

// Full reports whether the ring has wrapped (used by tests).
func (s *Sparkline) Full() bool {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.full
}
