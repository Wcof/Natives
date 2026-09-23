package quote

import "time"

// SessionPhase enumerates the CN A-share trading day state machine
// (阶段一 §1.3). Frequencies per phase trade off freshness vs. useless traffic.
type SessionPhase int

const (
	PhaseClosed    SessionPhase = iota // 夜间/周末：休眠监听
	PhaseAuction                        // 09:15–09:25 集合竞价
	PhaseBreak                          // 09:25–09:30 / 11:30–13:00 休整
	PhaseMorning                        // 09:30–11:30 连续竞价
	PhaseAfternoon                      // 13:00–15:00 连续竞价
	PhasePostMarket                     // 15:00 后：盘后（低频校验跨日）
)

// PollInterval returns the collection cadence for the phase.
func (p SessionPhase) PollInterval() time.Duration {
	switch p {
	case PhaseAuction:
		return 10 * time.Second
	case PhaseMorning, PhaseAfternoon:
		return 3 * time.Second
	case PhaseBreak, PhasePostMarket:
		return 30 * time.Second
	default:
		return 60 * time.Second
	}
}

// Streaming is true when estimates/ticks should be pushed to subscribers.
func (p SessionPhase) Streaming() bool {
	return p == PhaseAuction || p == PhaseMorning || p == PhaseAfternoon
}

// CurrentPhase maps Beijing wall-clock time onto the session state machine.
func CurrentPhase(now time.Time) SessionPhase {
	t := now.In(cstZone())
	switch t.Weekday() {
	case time.Saturday, time.Sunday:
		return PhaseClosed
	}
	hm := t.Hour()*100 + t.Minute()
	switch {
	case hm >= 915 && hm < 925:
		return PhaseAuction
	case hm >= 925 && hm < 930:
		return PhaseBreak
	case hm >= 930 && hm <= 1130:
		return PhaseMorning
	case hm > 1130 && hm < 1300:
		return PhaseBreak
	case hm >= 1300 && hm < 1500:
		return PhaseAfternoon
	case hm >= 1500 && hm < 1600:
		return PhasePostMarket
	default:
		return PhaseClosed
	}
}
