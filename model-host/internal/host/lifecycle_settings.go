package host

import (
	"encoding/json"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
)

// Resident controls whether the Model Host worker survives the last client
// disconnect. It belongs to the Host lifecycle, not to a proxy gateway state.
func (e *Engine) Resident() bool {
	snapshot, err := e.repo.Load()
	return err == nil && snapshot.Resident
}

func (e *Engine) setResident(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		Resident         *bool  `json:"resident"`
	}
	if json.Unmarshal(raw, &input) != nil || input.Resident == nil {
		return domain.Snapshot{}, invalid("常驻设置参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		snapshot.Resident = *input.Resident
		return nil
	})
	if err == nil && e.emit != nil {
		e.emit(nativeio.Response{OK: true, Event: "model_host_state_changed", Result: map[string]any{"snapshot": snapshot}})
	}
	return snapshot, err
}
