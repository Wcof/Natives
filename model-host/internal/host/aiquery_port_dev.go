//go:build natives_dev

package host

import (
	"os"
	"strconv"
)

// Test profiles must never query the real user's fixed read-only endpoint.
func aiQueryListenPort() int {
	if os.Getenv("NATIVES_MODEL_HOST_CONFIG_DIR") != "" {
		if port, err := strconv.Atoi(os.Getenv("NATIVES_MODEL_HOST_AIQUERY_PORT")); err == nil && port > 0 && port <= 65535 {
			return port
		}
	}
	return aiQueryPort
}
