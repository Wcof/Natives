package ws

import (
	"net/http"

	"natives/market-host/internal/ws/minimalws"
)

// upgrade wraps the minimal RFC6455 server handshake and frame codec.
func upgrade(w http.ResponseWriter, r *http.Request) (*minimalws.Conn, error) {
	return minimalws.Upgrade(w, r)
}

// Conn re-exports the minimal connection type for handler use.
type Conn = minimalws.Conn
