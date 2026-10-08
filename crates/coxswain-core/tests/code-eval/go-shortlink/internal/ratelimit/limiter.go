// Package ratelimit limits requests per client address with a token bucket.
package ratelimit

import (
	"net"
	"net/http"
	"sync"
	"time"
)

// Limiter gives each client address a bucket of burst tokens, refilled at rate a second.
type Limiter struct {
	mu      sync.Mutex
	rate    float64
	burst   float64
	buckets map[string]*bucket
}

type bucket struct {
	tokens float64
	last   time.Time
}

// New makes a limiter of rate requests a second with bursts of burst.
func New(rate, burst float64) *Limiter {
	return &Limiter{rate: rate, burst: burst, buckets: map[string]*bucket{}}
}

// Allow takes a token from the bucket of addr, if it has one.
func (l *Limiter) Allow(addr string) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	b, ok := l.buckets[addr]
	now := time.Now()
	if !ok {
		b = &bucket{tokens: l.burst, last: now}
		l.buckets[addr] = b
	}
	b.tokens = min(l.burst, b.tokens+now.Sub(b.last).Seconds()*l.rate)
	b.last = now
	if b.tokens < 1 {
		return false
	}
	b.tokens--
	return true
}

// Middleware answers 429 Too Many Requests to a client with no token left.
func (l *Limiter) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		host, _, _ := net.SplitHostPort(r.RemoteAddr)
		if !l.Allow(host) {
			http.Error(w, "slow down", http.StatusTooManyRequests)
			return
		}
		next.ServeHTTP(w, r)
	})
}
