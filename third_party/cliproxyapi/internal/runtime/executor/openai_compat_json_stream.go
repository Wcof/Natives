package executor

import (
	"encoding/json"
	"fmt"
	"io"
)

// Convert an ignored stream request without replaying or billing a second request.
func openAICompatJSONStream(body io.ReadCloser) ([]byte, error) {
	defer body.Close()
	data, err := io.ReadAll(io.LimitReader(body, 1024*1024+1))
	if err != nil {
		return nil, err
	}
	if len(data) > 1024*1024 {
		return nil, fmt.Errorf("upstream completion exceeds stream conversion limit")
	}
	var completion map[string]any
	if json.Unmarshal(data, &completion) != nil {
		return nil, fmt.Errorf("invalid upstream JSON completion")
	}
	choices, ok := completion["choices"].([]any)
	if !ok || len(choices) == 0 || len(choices) > 32 {
		return nil, fmt.Errorf("upstream JSON completion has no usable choices")
	}
	for _, raw := range choices {
		choice, ok := raw.(map[string]any)
		if !ok {
			return nil, fmt.Errorf("invalid upstream completion choice")
		}
		message, ok := choice["message"].(map[string]any)
		if !ok {
			return nil, fmt.Errorf("upstream completion has no message")
		}
		choice["delta"] = message
		delete(choice, "message")
	}
	completion["object"] = "chat.completion.chunk"
	data, err = json.Marshal(completion)
	if err != nil {
		return nil, err
	}
	return append(append([]byte("data: "), data...), []byte("\n\ndata: [DONE]\n\n")...), nil
}
