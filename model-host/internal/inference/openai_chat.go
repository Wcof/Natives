package inference

import (
	"encoding/json"
	"io"
	"strings"
)

type chatUsage struct {
	PromptTokens         int64 `json:"prompt_tokens"`
	CompletionTokens     int64 `json:"completion_tokens"`
	TotalTokens          int64 `json:"total_tokens"`
	PromptCacheHitTokens int64 `json:"prompt_cache_hit_tokens"`
	PromptTokensDetails  struct {
		CachedTokens int64 `json:"cached_tokens"`
	} `json:"prompt_tokens_details"`
	CompletionTokensDetails struct {
		ReasoningTokens int64 `json:"reasoning_tokens"`
	} `json:"completion_tokens_details"`
}

func buildOpenAIChat(request Request) (string, []byte, map[string]string, error) {
	endpoint, err := endpoint(request.BaseURL, "/chat/completions")
	if err != nil {
		return "", nil, nil, err
	}
	messages := make([]map[string]string, 0, 2)
	if strings.TrimSpace(request.SystemPrompt) != "" {
		messages = append(messages, map[string]string{"role": "system", "content": request.SystemPrompt})
	}
	messages = append(messages, map[string]string{"role": "user", "content": request.Text})
	body, err := json.Marshal(map[string]any{
		"model": request.ModelID, "stream": true, "max_tokens": request.MaxTokens,
		"messages": messages, "stream_options": map[string]bool{"include_usage": true},
	})
	if err != nil {
		return "", nil, nil, err
	}
	return endpoint, body, map[string]string{
		"Authorization": "Bearer " + request.APIKey,
		"Content-Type":  "application/json",
		"Accept":        "text/event-stream, application/json",
	}, nil
}

func parseOpenAIChatResponse(data []byte) Result {
	var payload struct {
		Choices []struct {
			Message struct {
				Content          json.RawMessage `json:"content"`
				ReasoningContent string          `json:"reasoning_content"`
				Reasoning        string          `json:"reasoning"`
			} `json:"message"`
			FinishReason string `json:"finish_reason"`
		} `json:"choices"`
		Usage *chatUsage      `json:"usage"`
		Error json.RawMessage `json:"error"`
	}
	if json.Unmarshal(data, &payload) != nil {
		return Result{Failure: FailureEmpty}
	}
	if len(payload.Error) > 0 && string(payload.Error) != "null" {
		return Result{Failure: FailureUpstream}
	}
	if len(payload.Choices) == 0 {
		return Result{Failure: FailureEmpty}
	}
	choice := payload.Choices[0]
	if choice.FinishReason == "length" {
		return Result{Failure: FailureTruncated, Usage: chatUsageFrom(payload.Usage)}
	}
	text := chatContent(choice.Message.Content)
	if strings.TrimSpace(text) == "" {
		return Result{Failure: FailureEmpty, Usage: chatUsageFrom(payload.Usage)}
	}
	return Result{Text: text, Usage: chatUsageFrom(payload.Usage)}
}

func parseOpenAIChatStream(reader io.Reader, progress func(string)) Result {
	scanner := newSSEScanner(reader)
	var answer strings.Builder
	var usage *Usage
	complete := false
	phase := "waiting"
	count := 0
	for {
		data, done, err := scanner.next()
		if err != nil {
			return Result{Failure: streamFailure(err), Usage: usage}
		}
		if done {
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
				FinishReason string `json:"finish_reason"`
			} `json:"choices"`
			Usage *chatUsage      `json:"usage"`
			Error json.RawMessage `json:"error"`
		}
		if json.Unmarshal([]byte(data), &chunk) != nil {
			return Result{Failure: FailureStream, Usage: usage}
		}
		if len(chunk.Error) > 0 && string(chunk.Error) != "null" {
			return Result{Failure: FailureUpstream, Usage: usage}
		}
		if chunk.Usage != nil {
			usage = chatUsageFrom(chunk.Usage)
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
			count += len([]rune(choice.Delta.Content))
			if count > MaxOutputRunes {
				return Result{Failure: FailureLength, Usage: usage}
			}
			answer.WriteString(choice.Delta.Content)
		}
		if next != phase {
			phase = next
			if progress != nil {
				progress(phase)
			}
		}
		if choice.FinishReason == "length" {
			return Result{Failure: FailureTruncated, Usage: usage}
		}
		if choice.FinishReason != "" {
			complete = true
		}
	}
	if !complete {
		return Result{Failure: FailureStream, Usage: usage}
	}
	if strings.TrimSpace(answer.String()) == "" {
		return Result{Failure: FailureEmpty, Usage: usage}
	}
	return Result{Text: answer.String(), Usage: usage}
}

func chatUsageFrom(usage *chatUsage) *Usage {
	if usage == nil {
		return nil
	}
	cached := usage.PromptTokensDetails.CachedTokens
	if cached == 0 {
		cached = usage.PromptCacheHitTokens
	}
	input := usage.PromptTokens - cached
	if input < 0 {
		input = 0
	}
	output := usage.CompletionTokens
	reasoning := usage.CompletionTokensDetails.ReasoningTokens
	if reasoning > output {
		reasoning = output
	}
	total := usage.TotalTokens
	if total == 0 {
		total = usage.PromptTokens + usage.CompletionTokens
	}
	return &Usage{
		InputTokens: input, OutputTokens: output,
		CacheReadTokens: cached, ReasoningTokens: reasoning, TotalTokens: total,
	}
}

func chatContent(raw json.RawMessage) string {
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text
	}
	var parts []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &parts) == nil {
		var builder strings.Builder
		for _, part := range parts {
			if part.Type == "" || part.Type == "text" || part.Type == "output_text" {
				builder.WriteString(part.Text)
			}
		}
		return builder.String()
	}
	return ""
}

func streamFailure(err error) string {
	if err == errStreamTooLarge {
		return FailureLength
	}
	if err == io.EOF {
		return FailureStream
	}
	return FailureStream
}
