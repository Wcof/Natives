package cliproxy

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"mime"
	"net/http"
	"strconv"
	"strings"

	"github.com/tidwall/gjson"
	"github.com/tidwall/sjson"
)

// Normalize only routing fields; prompts, tools and nested model fields are payload.
func normalizeGatewayModel(request *http.Request) error {
	if strings.HasPrefix(request.URL.Path, "/v1beta/models/natives-gateway/") {
		request.URL.Path = strings.Replace(request.URL.Path, "/models/natives-gateway/", "/models/", 1)
		request.URL.RawPath = ""
	}
	if request.Method != http.MethodPost || request.Body == nil {
		return nil
	}
	mediaType, _, _ := mime.ParseMediaType(request.Header.Get("Content-Type"))
	if mediaType != "" && mediaType != "application/json" {
		return nil
	}
	body, err := io.ReadAll(request.Body)
	_ = request.Body.Close()
	if err != nil {
		return err
	}
	if len(body) > 0 {
		if !json.Valid(body) {
			return fmt.Errorf("invalid JSON")
		}
		model := gjson.GetBytes(body, "model")
		if model.Type == gjson.String && strings.HasPrefix(model.Str, "natives-gateway/") {
			body, err = sjson.SetBytes(body, "model", strings.TrimPrefix(model.Str, "natives-gateway/"))
			if err != nil {
				return err
			}
		}
	}
	request.Body = io.NopCloser(bytes.NewReader(body))
	request.ContentLength = int64(len(body))
	request.Header.Set("Content-Length", strconv.Itoa(len(body)))
	return nil
}
