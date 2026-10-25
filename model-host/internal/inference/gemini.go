package inference

import (
	"encoding/json"
	"errors"
	"io"
	"strings"
)

type geminiUsage struct {
	PromptTokenCount        int64 `json:"promptTokenCount"`
	CandidatesTokenCount    int64 `json:"candidatesTokenCount"`
	CachedContentTokenCount int64 `json:"cachedContentTokenCount"`
	ThoughtsTokenCount      int64 `json:"thoughtsTokenCount"`
	TotalTokenCount         int64 `json:"totalTokenCount"`
}

type geminiPart struct {
	Text    string `json:"text"`
	Thought bool   `json:"thought"`
}

type geminiCandidate struct {
	Content struct {
		Parts []geminiPart `json:"parts"`
	} `json:"content"`
	FinishReason string `json:"finishReason"`
}

type geminiPayload struct {
	Candidates     []geminiCandidate `json:"candidates"`
	UsageMetadata  *geminiUsage      `json:"usageMetadata"`
	PromptFeedback struct {
		BlockReason string `json:"blockReason"`
	} `json:"promptFeedback"`
	Error *struct {
		Message string `json:"message"`
	} `json:"error"`
}

func buildGemini(request Request) (string, []byte, map[string]string, error) {
	endpoint, err := geminiEndpoint(request.BaseURL, request.ModelID, "streamGenerateContent")
	if err != nil {
		return "", nil, nil, err
	}
	body := map[string]any{
		"contents": []map[string]any{
			{"role": "user", "parts": []map[string]string{{"text": request.Text}}},
		},
		"generationConfig": map[string]any{"maxOutputTokens": request.MaxTokens},
	}
	if system := strings.TrimSpace(request.SystemPrompt); system != "" {
		body["system_instruction"] = map[string]any{"parts": []map[string]string{{"text": system}}}
	}
	data, err := json.Marshal(body)
	if err != nil {
		return "", nil, nil, err
	}
	return endpoint, data, map[string]string{
		"x-goog-api-key": request.APIKey,
		"Content-Type":   "application/json",
		"Accept":         "text/event-stream, application/json",
	}, nil
}

func parseGeminiResponse(data []byte) Result {
	var payload geminiPayload
	if json.Unmarshal(data, &payload) != nil {
		return Result{Failure: FailureEmpty}
	}
	if payload.Error != nil {
		return Result{Failure: FailureUpstream, Usage: geminiUsageFrom(payload.UsageMetadata)}
	}
	if payload.PromptFeedback.BlockReason != "" && len(payload.Candidates) == 0 {
		return Result{Failure: FailureUpstream, Usage: geminiUsageFrom(payload.UsageMetadata)}
	}
	if len(payload.Candidates) == 0 {
		return Result{Failure: FailureEmpty, Usage: geminiUsageFrom(payload.UsageMetadata)}
	}
	candidate := payload.Candidates[0]
	if candidate.FinishReason == "MAX_TOKENS" {
		return Result{Failure: FailureTruncated, Usage: geminiUsageFrom(payload.UsageMetadata)}
	}
	text := geminiText(candidate.Content.Parts)
	if strings.TrimSpace(text) == "" {
		return Result{Failure: FailureEmpty, Usage: geminiUsageFrom(payload.UsageMetadata)}
	}
	return Result{Text: text, Usage: geminiUsageFrom(payload.UsageMetadata)}
}

func parseGeminiStream(reader io.Reader, progress func(string)) Result {
	scanner := newSSEScanner(reader)
	var answer strings.Builder
	var usage *geminiUsage
	complete := false
	phase := "waiting"
	count := 0
	for {
		data, done, err := scanner.next()
		if err != nil {
			// Gemini SSE ends after the candidate carrying finishReason.
			if errors.Is(err, io.EOF) && complete {
				break
			}
			return Result{Failure: streamFailure(err), Usage: geminiUsageFrom(usage)}
		}
		if done {
			break
		}
		var payload geminiPayload
		if json.Unmarshal([]byte(data), &payload) != nil {
			return Result{Failure: FailureStream, Usage: geminiUsageFrom(usage)}
		}
		if payload.Error != nil {
			return Result{Failure: FailureUpstream, Usage: geminiUsageFrom(usage)}
		}
		if payload.UsageMetadata != nil {
			usage = payload.UsageMetadata
		}
		if len(payload.Candidates) == 0 {
			continue
		}
		candidate := payload.Candidates[0]
		for _, part := range candidate.Content.Parts {
			if part.Thought {
				if phase == "waiting" {
					phase = "reasoning"
					if progress != nil {
						progress(phase)
					}
				}
				continue
			}
			if part.Text == "" {
				continue
			}
			count += len([]rune(part.Text))
			if count > MaxOutputRunes {
				return Result{Failure: FailureLength, Usage: geminiUsageFrom(usage)}
			}
			answer.WriteString(part.Text)
			if phase != "writing" {
				phase = "writing"
				if progress != nil {
					progress(phase)
				}
			}
		}
		if candidate.FinishReason == "MAX_TOKENS" {
			return Result{Failure: FailureTruncated, Usage: geminiUsageFrom(usage)}
		}
		if candidate.FinishReason != "" {
			complete = true
		}
	}
	if !complete {
		return Result{Failure: FailureStream, Usage: geminiUsageFrom(usage)}
	}
	if strings.TrimSpace(answer.String()) == "" {
		return Result{Failure: FailureEmpty, Usage: geminiUsageFrom(usage)}
	}
	return Result{Text: answer.String(), Usage: geminiUsageFrom(usage)}
}

func geminiText(parts []geminiPart) string {
	var builder strings.Builder
	for _, part := range parts {
		if !part.Thought {
			builder.WriteString(part.Text)
		}
	}
	return builder.String()
}

func geminiUsageFrom(usage *geminiUsage) *Usage {
	if usage == nil {
		return nil
	}
	cached := usage.CachedContentTokenCount
	input := usage.PromptTokenCount - cached
	if input < 0 {
		input = 0
	}
	// Gemini reports thinking tokens separately from candidate tokens; both are
	// billed as output. Reasoning remains an informational subset.
	output := usage.CandidatesTokenCount + usage.ThoughtsTokenCount
	total := usage.TotalTokenCount
	if total == 0 {
		total = usage.PromptTokenCount + output
	}
	return &Usage{
		InputTokens: input, OutputTokens: output, CacheReadTokens: cached,
		ReasoningTokens: usage.ThoughtsTokenCount, TotalTokens: total,
	}
}
