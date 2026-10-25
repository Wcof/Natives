package inference

import (
	"encoding/json"
	"errors"
	"io"
	"strings"
)

type anthropicUsage struct {
	InputTokens              int64 `json:"input_tokens"`
	OutputTokens             int64 `json:"output_tokens"`
	CacheReadInputTokens     int64 `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int64 `json:"cache_creation_input_tokens"`
}

type anthropicBlock struct {
	Type     string `json:"type"`
	Text     string `json:"text"`
	Thinking string `json:"thinking"`
}

func buildAnthropic(request Request) (string, []byte, map[string]string, error) {
	endpoint, err := endpoint(request.BaseURL, "/messages")
	if err != nil {
		return "", nil, nil, err
	}
	body := map[string]any{
		"model":      request.ModelID,
		"max_tokens": request.MaxTokens,
		"stream":     true,
		"messages": []map[string]any{
			{"role": "user", "content": []map[string]string{{"type": "text", "text": request.Text}}},
		},
	}
	if system := strings.TrimSpace(request.SystemPrompt); system != "" {
		body["system"] = system
	}
	data, err := json.Marshal(body)
	if err != nil {
		return "", nil, nil, err
	}
	return endpoint, data, map[string]string{
		"x-api-key":         request.APIKey,
		"anthropic-version": "2023-06-01",
		"Content-Type":      "application/json",
		"Accept":            "text/event-stream, application/json",
	}, nil
}

func parseAnthropicResponse(data []byte) Result {
	var payload struct {
		Content    []anthropicBlock `json:"content"`
		Usage      *anthropicUsage  `json:"usage"`
		StopReason string           `json:"stop_reason"`
		Error      *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if json.Unmarshal(data, &payload) != nil {
		return Result{Failure: FailureEmpty}
	}
	if payload.Error != nil {
		return Result{Failure: FailureUpstream, Usage: anthropicUsageFrom(payload.Usage)}
	}
	text := anthropicText(payload.Content)
	if payload.StopReason == "max_tokens" {
		return Result{Failure: FailureTruncated, Usage: anthropicUsageFrom(payload.Usage)}
	}
	if strings.TrimSpace(text) == "" {
		return Result{Failure: FailureEmpty, Usage: anthropicUsageFrom(payload.Usage)}
	}
	return Result{Text: text, Usage: anthropicUsageFrom(payload.Usage)}
}

func parseAnthropicStream(reader io.Reader, progress func(string)) Result {
	scanner := newSSEScanner(reader)
	var answer strings.Builder
	var usage *anthropicUsage
	complete := false
	phase := "waiting"
	count := 0
	for {
		data, done, err := scanner.next()
		if err != nil {
			// Anthropic streams terminate at message_stop rather than [DONE].
			if errors.Is(err, io.EOF) && complete {
				break
			}
			return Result{Failure: streamFailure(err), Usage: anthropicUsageFrom(usage)}
		}
		if done {
			break
		}
		var event struct {
			Type    string `json:"type"`
			Message *struct {
				Usage *anthropicUsage `json:"usage"`
			} `json:"message"`
			Delta *struct {
				Type       string `json:"type"`
				Text       string `json:"text"`
				Thinking   string `json:"thinking"`
				StopReason string `json:"stop_reason"`
			} `json:"delta"`
			Usage *anthropicUsage `json:"usage"`
			Error *struct {
				Message string `json:"message"`
			} `json:"error"`
		}
		if json.Unmarshal([]byte(data), &event) != nil {
			return Result{Failure: FailureStream, Usage: anthropicUsageFrom(usage)}
		}
		switch event.Type {
		case "message_start":
			if event.Message != nil && event.Message.Usage != nil {
				usage = event.Message.Usage
			}
		case "content_block_delta":
			if event.Delta == nil {
				continue
			}
			if event.Delta.Type == "thinking_delta" && phase == "waiting" {
				phase = "reasoning"
				if progress != nil {
					progress(phase)
				}
			}
			if event.Delta.Type == "text_delta" && event.Delta.Text != "" {
				count += len([]rune(event.Delta.Text))
				if count > MaxOutputRunes {
					return Result{Failure: FailureLength, Usage: anthropicUsageFrom(usage)}
				}
				answer.WriteString(event.Delta.Text)
				if phase != "writing" {
					phase = "writing"
					if progress != nil {
						progress(phase)
					}
				}
			}
			if event.Delta.Thinking != "" && phase == "waiting" {
				phase = "reasoning"
				if progress != nil {
					progress(phase)
				}
			}
		case "message_delta":
			if event.Usage != nil {
				if usage == nil {
					usage = event.Usage
				} else {
					usage.OutputTokens = event.Usage.OutputTokens
				}
			}
			if event.Delta != nil && event.Delta.StopReason == "max_tokens" {
				return Result{Failure: FailureTruncated, Usage: anthropicUsageFrom(usage)}
			}
		case "message_stop":
			complete = true
		case "error":
			return Result{Failure: FailureUpstream, Usage: anthropicUsageFrom(usage)}
		}
	}
	if !complete {
		return Result{Failure: FailureStream, Usage: anthropicUsageFrom(usage)}
	}
	if strings.TrimSpace(answer.String()) == "" {
		return Result{Failure: FailureEmpty, Usage: anthropicUsageFrom(usage)}
	}
	return Result{Text: answer.String(), Usage: anthropicUsageFrom(usage)}
}

func anthropicText(blocks []anthropicBlock) string {
	var builder strings.Builder
	for _, block := range blocks {
		if block.Type == "" || block.Type == "text" {
			builder.WriteString(block.Text)
		}
	}
	return builder.String()
}

func anthropicUsageFrom(usage *anthropicUsage) *Usage {
	if usage == nil {
		return nil
	}
	output := usage.OutputTokens
	return &Usage{
		InputTokens:      usage.InputTokens,
		OutputTokens:     output,
		CacheReadTokens:  usage.CacheReadInputTokens,
		CacheWriteTokens: usage.CacheCreationInputTokens,
		TotalTokens:      usage.InputTokens + output + usage.CacheReadInputTokens + usage.CacheCreationInputTokens,
	}
}
