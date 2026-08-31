package netpolicy

import (
	"context"
	"errors"
	"net"
	"net/http"
	"strings"
	"time"
)

func NewTransport(privateHosts map[string]bool) *http.Transport {
	dialer := &net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}
	return &http.Transport{
		ForceAttemptHTTP2: true,
		DialContext: func(ctx context.Context, network, address string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(address)
			if err != nil {
				return nil, err
			}
			addresses, err := net.DefaultResolver.LookupIPAddr(ctx, host)
			if err != nil {
				return nil, err
			}
			allowPrivate := privateHosts[strings.ToLower(strings.TrimSuffix(host, "."))]
			for _, address := range addresses {
				if permitted(address.IP, allowPrivate) {
					return dialer.DialContext(ctx, network, net.JoinHostPort(address.IP.String(), port))
				}
			}
			return nil, errors.New("destination is blocked by network policy")
		},
	}
}

func permitted(ip net.IP, allowPrivate bool) bool {
	if ip.IsUnspecified() || ip.IsMulticast() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() {
		return false
	}
	if ip.IsLoopback() || ip.IsPrivate() {
		return allowPrivate
	}
	return true
}
