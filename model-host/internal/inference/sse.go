package inference

import (
	"bufio"
	"errors"
	"io"
	"strings"
)

var errStreamTooLarge = errors.New("stream exceeds limit")

type sseScanner struct {
	limited *io.LimitedReader
	scanner *bufio.Scanner
}

func newSSEScanner(reader io.Reader) *sseScanner {
	limited := &io.LimitedReader{R: reader, N: MaxStreamBytes + 1}
	scanner := bufio.NewScanner(limited)
	scanner.Buffer(make([]byte, 4096), MaxLineBytes)
	return &sseScanner{limited: limited, scanner: scanner}
}

// next returns the payload of the next data line. done is true on [DONE].
func (s *sseScanner) next() (data string, done bool, err error) {
	for s.scanner.Scan() {
		line := s.scanner.Text()
		if !strings.HasPrefix(line, "data:") {
			continue
		}
		payload := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
		if payload == "[DONE]" {
			return "", true, nil
		}
		if payload == "" {
			continue
		}
		return payload, false, nil
	}
	if err := s.scanner.Err(); err != nil {
		return "", false, err
	}
	if s.limited.N <= 0 {
		return "", false, errStreamTooLarge
	}
	return "", false, io.EOF
}
