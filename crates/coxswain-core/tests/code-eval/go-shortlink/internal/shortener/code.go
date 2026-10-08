// Package shortener makes the short codes of links.
package shortener

import (
	"crypto/rand"
	"math/big"
)

// Length is the number of characters of a short code.
const Length = 7

const alphabet = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"

// NewCode returns a random code of Length characters from the base62 alphabet:
// 62^7, about 3.5 trillion codes.
func NewCode() (string, error) {
	b := make([]byte, Length)
	for i := range b {
		n, err := rand.Int(rand.Reader, big.NewInt(int64(len(alphabet))))
		if err != nil {
			return "", err
		}
		b[i] = alphabet[n.Int64()]
	}
	return string(b), nil
}

// Valid reports whether s could be a code.
func Valid(s string) bool {
	if len(s) != Length {
		return false
	}
	for _, c := range s {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z') {
			return false
		}
	}
	return true
}
