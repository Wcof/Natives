package host

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"time"
	"unicode/utf8"
)

func inferenceTransportFailure(ctx context.Context) string {
	if errors.Is(context.Cause(ctx), context.DeadlineExceeded) {
		return "INFERENCE_TIMEOUT"
	}
	if ctx.Err() != nil {
		return "INFERENCE_CANCELLED"
	}
	return "INFERENCE_CONNECTION"
}

type inferenceActivityReader struct {
	reader io.Reader
	idle   *time.Timer
}

func (r inferenceActivityReader) Read(p []byte) (int, error) {
	n, err := r.reader.Read(p)
	if n > 0 {
		r.idle.Reset(30 * time.Second)
	}
	return n, err
}

func readInferenceResponse(ctx context.Context, response *http.Response, idle *time.Timer, progress func(string)) (string, string) {
	reader := inferenceActivityReader{reader: response.Body, idle: idle}
	if !strings.Contains(strings.ToLower(response.Header.Get("Content-Type")), "text/event-stream") {
		data, err := io.ReadAll(io.LimitReader(reader, 65537))
		if err != nil {
			return "", inferenceTransportFailure(ctx)
		}
		if len(data) > 65536 {
			return "", "INFERENCE_LENGTH"
		}
		var parsed struct {
			Choices []struct {
				Message struct {
					Content string `json:"content"`
				} `json:"message"`
				Finish string `json:"finish_reason"`
			} `json:"choices"`
		}
		if json.Unmarshal(data, &parsed) != nil || len(parsed.Choices) == 0 || parsed.Choices[0].Message.Content == "" {
			return "", "INFERENCE_EMPTY"
		}
		if parsed.Choices[0].Finish == "length" {
			return "", "INFERENCE_TRUNCATED"
		}
		answer := parsed.Choices[0].Message.Content
		if utf8.RuneCountInString(answer) > 8000 {
			return "", "INFERENCE_LENGTH"
		}
		return answer, ""
	}
	limited := &io.LimitedReader{R: reader, N: 1024*1024 + 1}
	scanner := bufio.NewScanner(limited)
	scanner.Buffer(make([]byte, 4096), 65536)
	var answer strings.Builder
	count := 0
	phase := "waiting"
	complete := false
	for scanner.Scan() {
		line := scanner.Text()
		if !strings.HasPrefix(line, "data:") {
			continue
		}
		data := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
		if data == "[DONE]" {
			complete = true
			break
		}
		var chunk struct {
			Choices []struct {
				Delta struct {
					Content          string `json:"content"`
					Reasoning        string `json:"reasoning"`
					ReasoningContent string `json:"reasoning_content"`
				} `json:"delta"`
				Finish string `json:"finish_reason"`
			} `json:"choices"`
			Error json.RawMessage `json:"error"`
		}
		if json.Unmarshal([]byte(data), &chunk) != nil {
			return "", "INFERENCE_STREAM"
		}
		if len(chunk.Error) > 0 && string(chunk.Error) != "null" {
			return "", "INFERENCE_UPSTREAM"
		}
		if len(chunk.Choices) == 0 {
			continue
		}
		choice := chunk.Choices[0]
		next := phase
		if choice.Delta.Reasoning != "" || choice.Delta.ReasoningContent != "" {
			next = "reasoning"
		}
		if choice.Delta.Content != "" {
			next = "writing"
			count += utf8.RuneCountInString(choice.Delta.Content)
			if count > 8000 {
				return "", "INFERENCE_LENGTH"
			}
			answer.WriteString(choice.Delta.Content)
		}
		if next != phase {
			phase = next
			if progress != nil {
				progress(phase)
			}
		}
		if choice.Finish == "length" {
			return "", "INFERENCE_TRUNCATED"
		}
		if choice.Finish != "" {
			complete = true
		}
	}
	if ctx.Err() != nil {
		return "", inferenceTransportFailure(ctx)
	}
	if limited.N <= 0 {
		return "", "INFERENCE_LENGTH"
	}
	if scanner.Err() != nil || !complete {
		return "", "INFERENCE_STREAM"
	}
	if answer.Len() == 0 {
		return "", "INFERENCE_EMPTY"
	}
	return answer.String(), ""
}
