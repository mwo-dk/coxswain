// Package store keeps links in PostgreSQL.
package store

import (
	"context"
	"errors"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrNotFound is returned for a code no link has.
var ErrNotFound = errors.New("no such link")

// Store is a pool of connections to the database.
type Store struct {
	pool *pgxpool.Pool
}

// Open connects to the database at url.
func Open(ctx context.Context, url string) (*Store, error) {
	pool, err := pgxpool.New(ctx, url)
	if err != nil {
		return nil, err
	}
	return &Store{pool: pool}, nil
}

// Save stores the link under code.
func (s *Store) Save(ctx context.Context, code, target string) error {
	_, err := s.pool.Exec(ctx, "INSERT INTO links(code, target) VALUES ($1, $2)", code, target)
	return err
}

// Target looks up the link of code and counts the visit.
func (s *Store) Target(ctx context.Context, code string) (string, error) {
	var target string
	err := s.pool.QueryRow(ctx, "UPDATE links SET visits = visits + 1 WHERE code = $1 RETURNING target", code).Scan(&target)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", ErrNotFound
	}
	return target, err
}
