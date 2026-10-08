// Command server runs the link shortener's HTTP server.
package main

import (
	"context"
	"net/http"
	"os"

	"go.uber.org/zap"

	"github.com/example/shortlink/internal/ratelimit"
	"github.com/example/shortlink/internal/store"
	"github.com/example/shortlink/internal/web"
)

func main() {
	log, _ := zap.NewProduction()
	defer log.Sync()
	db, err := store.Open(context.Background(), os.Getenv("DATABASE_URL"))
	if err != nil {
		log.Fatal("database", zap.Error(err))
	}
	limiter := ratelimit.New(10, 20)
	router := web.Router(db, limiter, log)
	log.Info("listening", zap.String("addr", ":8080"))
	if err := http.ListenAndServe(":8080", router); err != nil {
		log.Fatal("server", zap.Error(err))
	}
}
