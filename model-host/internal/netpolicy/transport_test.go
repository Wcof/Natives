package netpolicy

import (
	"net"
	"testing"
)

func TestNetworkPolicy(t *testing.T) {
	for _, test := range []struct {
		ip      string
		allowed bool
		want    bool
	}{
		{"8.8.8.8", false, true},
		{"127.0.0.1", false, false},
		{"127.0.0.1", true, true},
		{"192.168.1.2", false, false},
		{"192.168.1.2", true, true},
		{"169.254.1.2", true, false},
		{"0.0.0.0", true, false},
	} {
		if got := permitted(net.ParseIP(test.ip), test.allowed); got != test.want {
			t.Fatalf("permitted(%s, %v) = %v", test.ip, test.allowed, got)
		}
	}
}
