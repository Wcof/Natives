package nativeio

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sync"
)

const maxMessageBytes = 1024 * 1024

type Request struct {
	ID     string          `json:"id"`
	Method string          `json:"method"`
	Params json.RawMessage `json:"params"`
}

type Response struct {
	ID        string `json:"id,omitempty"`
	OK        bool   `json:"ok"`
	Result    any    `json:"result,omitempty"`
	Error     string `json:"error,omitempty"`
	ErrorCode string `json:"errorCode,omitempty"`
	Event     string `json:"event,omitempty"`
}

type Writer struct {
	out io.Writer
	mu  sync.Mutex
}

func NewWriter(out io.Writer) *Writer { return &Writer{out: out} }

func Read(r io.Reader) (Request, error) {
	var size uint32
	if err := binary.Read(r, binary.LittleEndian, &size); err != nil {
		return Request{}, err
	}
	if size == 0 || size > maxMessageBytes {
		return Request{}, fmt.Errorf("invalid native message size")
	}
	payload := make([]byte, size)
	if _, err := io.ReadFull(r, payload); err != nil {
		return Request{}, err
	}
	var request Request
	if err := json.Unmarshal(payload, &request); err != nil {
		return Request{}, errors.New("invalid native message")
	}
	if request.ID == "" || request.Method == "" {
		return Request{}, errors.New("invalid native request")
	}
	return request, nil
}

func (w *Writer) Write(response Response) error {
	payload, err := json.Marshal(response)
	if err != nil {
		return err
	}
	if len(payload) > maxMessageBytes {
		return errors.New("native response exceeds size limit")
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	if err = binary.Write(w.out, binary.LittleEndian, uint32(len(payload))); err != nil {
		return err
	}
	_, err = w.out.Write(payload)
	return err
}
