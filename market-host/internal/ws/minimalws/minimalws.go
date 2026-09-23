// Package minimalws is a dependency-free RFC6455 server-side subset:
// handshake + unfragmented text frames both ways + close. It exists solely
// to keep market-host a single binary with no third-party websocket module
// (工程规范：非必要不引入打包依赖).
package minimalws

import (
	"bufio"
	"crypto/sha1"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"sync"
)

const wsGUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

// Conn is a minimal websocket server connection.
type Conn struct {
	rwc    *bufio.ReadWriter
	raw    net.Conn
	wmu    sync.Mutex
	closed bool
}

// Upgrade performs the HTTP→WS handshake.
func Upgrade(w http.ResponseWriter, r *http.Request) (*Conn, error) {
	if !strings.EqualFold(r.Header.Get("Upgrade"), "websocket") ||
		!strings.Contains(strings.ToLower(r.Header.Get("Connection")), "upgrade") {
		http.Error(w, "expected websocket upgrade", http.StatusBadRequest)
		return nil, errors.New("not a websocket request")
	}
	key := r.Header.Get("Sec-WebSocket-Key")
	if key == "" {
		http.Error(w, "missing Sec-WebSocket-Key", http.StatusBadRequest)
		return nil, errors.New("missing key")
	}
	h := sha1.New()
	h.Write([]byte(key + wsGUID))
	accept := base64.StdEncoding.EncodeToString(h.Sum(nil))
	hj, ok := w.(http.Hijacker)
	if !ok {
		http.Error(w, "hijack unsupported", http.StatusInternalServerError)
		return nil, errors.New("hijack unsupported")
	}
	raw, rw, err := hj.Hijack()
	if err != nil {
		return nil, err
	}
	fmt.Fprintf(rw, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n", accept)
	if err := rw.Flush(); err != nil {
		raw.Close()
		return nil, err
	}
	return &Conn{rwc: rw, raw: raw}, nil
}

// Read returns the next text frame payload; close frames end the loop.
func (c *Conn) Read() ([]byte, error) {
	for {
		fin, op, payload, err := c.readFrame()
		if err != nil {
			return nil, err
		}
		switch op {
		case 0x8: // close
			c.writeFrame(0x8, nil)
			return nil, io.EOF
		case 0x9: // ping → pong
			c.writeFrame(0xA, payload)
		case 0x1, 0x0: // text / continuation
			if fin {
				return payload, nil
			}
			return nil, errors.New("fragmented frames unsupported")
		default:
			// ignore binary/other
		}
	}
}

// Write sends one unmasked text frame.
func (c *Conn) Write(data []byte) error {
	c.wmu.Lock()
	defer c.wmu.Unlock()
	if c.closed {
		return errors.New("closed")
	}
	return c.writeFrame(0x1, data)
}

// Close sends a close frame and drops the TCP connection.
func (c *Conn) Close() error {
	c.wmu.Lock()
	defer c.wmu.Unlock()
	if !c.closed {
		c.closed = true
		_ = c.writeFrame(0x8, nil)
	}
	return c.raw.Close()
}

func (c *Conn) readFrame() (fin bool, op byte, payload []byte, err error) {
	var hdr [2]byte
	if _, err = io.ReadFull(c.rwc, hdr[:]); err != nil {
		return
	}
	fin = hdr[0]&0x80 != 0
	op = hdr[0] & 0x0F
	masked := hdr[1]&0x80 != 0
	plen := uint64(hdr[1] & 0x7F)
	switch plen {
	case 126:
		var ext [2]byte
		if _, err = io.ReadFull(c.rwc, ext[:]); err != nil {
			return
		}
		plen = uint64(binary.BigEndian.Uint16(ext[:]))
	case 127:
		var ext [8]byte
		if _, err = io.ReadFull(c.rwc, ext[:]); err != nil {
			return
		}
		plen = binary.BigEndian.Uint64(ext[:])
	}
	if plen > 1<<20 {
		err = errors.New("frame too large")
		return
	}
	var mask [4]byte
	if masked {
		if _, err = io.ReadFull(c.rwc, mask[:]); err != nil {
			return
		}
	}
	payload = make([]byte, plen)
	if _, err = io.ReadFull(c.rwc, payload); err != nil {
		return
	}
	if masked {
		for i := range payload {
			payload[i] ^= mask[i%4]
		}
	}
	return
}

func (c *Conn) writeFrame(op byte, payload []byte) error {
	hdr := []byte{0x80 | op}
	n := len(payload)
	switch {
	case n < 126:
		hdr = append(hdr, byte(n))
	case n <= 0xFFFF:
		hdr = append(hdr, 126, byte(n>>8), byte(n))
	default:
		ext := make([]byte, 8)
		binary.BigEndian.PutUint64(ext, uint64(n))
		hdr = append(hdr, 127)
		hdr = append(hdr, ext...)
	}
	if _, err := c.rwc.Write(hdr); err != nil {
		return err
	}
	if _, err := c.rwc.Write(payload); err != nil {
		return err
	}
	return c.rwc.Flush()
}
