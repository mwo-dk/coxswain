// Package web is the HTTP API: making short links and following them.
package web

import (
	"encoding/json"
	"net/http"

	"github.com/go-chi/chi/v5"
	"go.uber.org/zap"

	"github.com/example/shortlink/internal/ratelimit"
	"github.com/example/shortlink/internal/shortener"
	"github.com/example/shortlink/internal/store"
)

// Router mounts the routes: POST /links makes a link, GET /{code} follows one.
func Router(db *store.Store, limiter *ratelimit.Limiter, log *zap.Logger) http.Handler {
	r := chi.NewRouter()
	r.Use(limiter.Middleware)
	r.Post("/links", func(w http.ResponseWriter, req *http.Request) { create(w, req, db, log) })
	r.Get("/{code}", func(w http.ResponseWriter, req *http.Request) { follow(w, req, db) })
	return r
}

func create(w http.ResponseWriter, req *http.Request, db *store.Store, log *zap.Logger) {
	var body struct {
		Target string `json:"target"`
	}
	if err := json.NewDecoder(req.Body).Decode(&body); err != nil || body.Target == "" {
		http.Error(w, "a target is needed", http.StatusBadRequest)
		return
	}
	code, err := shortener.NewCode()
	if err == nil {
		err = db.Save(req.Context(), code, body.Target)
	}
	if err != nil {
		log.Error("save", zap.Error(err))
		http.Error(w, "could not save", http.StatusInternalServerError)
		return
	}
	json.NewEncoder(w).Encode(map[string]string{"code": code})
}

// follow resolves a short link: it redirects to the target with 301 Moved Permanently.
func follow(w http.ResponseWriter, req *http.Request, db *store.Store) {
	code := chi.URLParam(req, "code")
	if !shortener.Valid(code) {
		http.NotFound(w, req)
		return
	}
	target, err := db.Target(req.Context(), code)
	if err != nil {
		http.NotFound(w, req)
		return
	}
	http.Redirect(w, req, target, http.StatusMovedPermanently)
}
