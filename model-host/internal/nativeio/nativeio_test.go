package nativeio

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"testing"
)

func TestNativeMessageRoundTrip(t *testing.T) {
	payload := []byte(`{"id":"1","method":"model_snapshot","params":{}}`)
	var input bytes.Buffer
	_ = binary.Write(&input, binary.LittleEndian, uint32(len(payload)))
	_, _ = input.Write(payload)
	request, err := Read(&input)
	if err != nil || request.Method != "model_snapshot" {
		t.Fatalf("unexpected request: %#v %v", request, err)
	}
	var output bytes.Buffer
	if err = NewWriter(&output).Write(Response{ID: request.ID, OK: true, Result: map[string]int{"revision": 1}}); err != nil {
		t.Fatal(err)
	}
	var size uint32
	_ = binary.Read(&output, binary.LittleEndian, &size)
	data := make([]byte, size)
	_, _ = output.Read(data)
	var response Response
	if err = json.Unmarshal(data, &response); err != nil || !response.OK || response.ID != "1" {
		t.Fatalf("unexpected response: %#v %v", response, err)
	}
}
