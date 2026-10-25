package inference

import (
	"encoding/json"
	"errors"
	"io"
	"strings"
)

type responsesUsage struct {
	InputTokens        int64 `json:"input_tokens"`
	OutputTokens       int64 `json:"output_tokens"`
	TotalTokens        int64 `json:"total_tokens"`
	InputTokensDetails struct {
		CachedTokens int64 `json:"cached_tokens"`
	} `json:"input_tokens_details"`
	OutputTokensDetails struct {
		ReasoningTokens int64 `json:"reasoning_tokens"`
	} `json:"output_tokens_details"`
}

type responsesOutput struct {
	Type    string `json:"type"`
	Content []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	} `json:"content"`
}

type responsesEnvelope struct {
	Status            string            `json:"status"`
	Output            []responsesOutput `json:"output"`
	OutputText        string            `json:"output_text"`
	Usage             *responsesUsage   `json:"usage"`
	IncompleteDetails struct {
		Reason string `json:"reason"`
	} `json:"incomplete_details"`
	Error *struct {
		Message string `json:"message"`
	} `json:"error"`
}

func buildOpenAIResponses(request Request) (string, []byte, map[string]string, error) {
	endpoint, err := endpoint(request.BaseURL, "/responses")
	if err != nil {
		return "", nil, nil, err
	}
	input := make([]map[string]any, 0, 2)
	if strings.TrimSpace(request.SystemPrompt) != "" {
		input = append(input, map[string]any{
			"role":    "system",
			"content": []map[string]string{{"type": "input_text", "text": request.SystemPrompt}},
		})
	}
	input = append(input, map[string]any{
		"role":    "user",
		"content": []map[string]string{{"type": "input_text", "text": request.Text}},
	})
	body, err := json.Marshal(map[string]any{
		"model":             request.ModelID,
		"input":             input,
		"stream":            true,
		"max_output_tokens": request.MaxTokens,
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

func parseOpenAIResponsesResponse(data []byte) Result {
	var payload responsesEnvelope
	if json.Unmarshal(data, &payload) != nil {
		return Result{Failure: FailureEmpty}
	}
	if payload.Error != nil || payload.Status == "failed" {
		return Result{Failure: FailureUpstream, Usage: responsesUsageFrom(payload.Usage)}
	}
	if payload.IncompleteDetails.Reason == "max_output_tokens" {
		return Result{Failure: FailureTruncated, Usage: responsesUsageFrom(payload.Usage)}
	}
	if payload.Status != "completed" {
		return Result{Failure: FailureStream, Usage: responsesUsageFrom(payload.Usage)}
	}
	text := proxyResponsesText(payload)
	if strings.TrimSpace(text) == "" {
		return Result{Failure: FailureEmpty, Usage: responsesUsageFrom(payload.Usage)}
	}
	return Result{Text: text, Usage: responsesUsageFrom(payload.Usage)}
}

func parseOpenAIResponsesStream(reader io.Reader, progress func(string)) Result {
	scanner := newSSEScanner(reader)
	var answer strings.Builder
	var usage *Usage
	var final *responsesEnvelope
	complete := false
	phase := "waiting"
	count := 0
	for {
		data, done, err := scanner.next()
		if err != nil {
			// Responses streams end at the terminal event without [DONE]; EOF is
			// only acceptable after response.completed/incomplete was observed.
			if errors.Is(err, io.EOF) && complete {
				break
			}
			return Result{Failure: streamFailure(err), Usage: usage}
		}
		if done {
			break
		}
		var event struct {
			Type     string             `json:"type"`
			Delta    string             `json:"delta"`
			Response *responsesEnvelope `json:"response"`
			Error    *struct {
				Message string `json:"message"`
			} `json:"error"`
		}
		if json.Unmarshal([]byte(data), &event) != nil {
			return Result{Failure: FailureStream, Usage: usage}
		}
		switch event.Type {
		case "response.output_text.delta":
			if event.Delta != "" {
				count += len([]rune(event.Delta))
				if count > MaxOutputRunes {
					return Result{Failure: FailureLength, Usage: usage}
				}
				answer.WriteString(event.Delta)
				if phase != "writing" {
					phase = "writing"
					if progress != nil {
						progress(phase)
					}
				}
			}
		case "response.reasoning_summary_text.delta", "response.reasoning_text.delta":
			if phase == "waiting" {
				phase = "reasoning"
				if progress != nil {
					progress(phase)
				}
			}
		case "response.completed", "response.incomplete":
			if event.Response == nil {
				return Result{Failure: FailureStream, Usage: usage}
			}
			complete = true
			final = event.Response
			if final.Usage != nil {
				usage = responsesUsageFrom(final.Usage)
			}
			if final.IncompleteDetails.Reason == "max_output_tokens" {
				return Result{Failure: FailureTruncated, Usage: usage}
			}
			if event.Type != "response.completed" || final.Status != "completed" || final.Error != nil {
				return Result{Failure: FailureStream, Usage: usage}
			}
		case "response.failed", "error":
			if event.Response != nil && event.Response.Usage != nil {
				usage = responsesUsageFrom(event.Response.Usage)
			}
			return Result{Failure: FailureUpstream, Usage: usage}
		}
		if event.Response != nil && event.Response.Usage != nil {
			usage = responsesUsageFrom(event.Response.Usage)
		}
	}
	if final != nil {
		if final.Usage != nil {
			usage = responsesUsageFrom(final.Usage)
		}
		if final.IncompleteDetails.Reason == "max_output_tokens" {
			return Result{Failure: FailureTruncated, Usage: usage}
		}
		if answer.Len() == 0 {
			answer.WriteString(proxyResponsesText(*final))
		}
	}
	if len([]rune(answer.String())) > MaxOutputRunes {
		return Result{Failure: FailureLength, Usage: usage}
	}
	if !complete {
		return Result{Failure: FailureStream, Usage: usage}
	}
	if strings.TrimSpace(answer.String()) == "" {
		return Result{Failure: FailureEmpty, Usage: usage}
	}
	return Result{Text: answer.String(), Usage: usage}
}

func proxyResponsesText(payload responsesEnvelope) string {
	var builder strings.Builder
	for _, output := range payload.Output {
		if output.Type != "" && output.Type != "message" {
			continue
		}
		for _, content := range output.Content {
			if content.Type == "" || content.Type == "output_text" || content.Type == "text" {
				builder.WriteString(content.Text)
			}
		}
	}
	if builder.Len() == 0 {
		builder.WriteString(payload.OutputText)
	}
	return builder.String()
}

func responsesUsageFrom(usage *responsesUsage) *Usage {
	if usage == nil {
		return nil
	}
	cached := usage.InputTokensDetails.CachedTokens
	input := usage.InputTokens - cached
	if input < 0 {
		input = 0
	}
	output := usage.OutputTokens
	reasoning := usage.OutputTokensDetails.ReasoningTokens
	if reasoning > output {
		reasoning = output
	}
	total := usage.TotalTokens
	if total == 0 {
		total = usage.InputTokens + usage.OutputTokens
	}
	return &Usage{
		InputTokens: input, OutputTokens: output, CacheReadTokens: cached,
		ReasoningTokens: reasoning, TotalTokens: total,
	}
}
