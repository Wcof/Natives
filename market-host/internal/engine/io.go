package engine

import (
	"io"
	"strings"
)

// readAll drains an HTTP body into a strings.Builder (GBK decode happens
// later inside the parsers so raw bytes stay intact in tests).
func readAll(r io.Reader, b *strings.Builder) (int64, error) {
	buf := make([]byte, 32*1024)
	var total int64
	for {
		n, err := r.Read(buf)
		if n > 0 {
			total += int64(n)
			b.Write(buf[:n])
		}
		if err == io.EOF {
			return total, nil
		}
		if err != nil {
			return total, err
		}
	}
}
