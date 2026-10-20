//go:build !natives_dev

package host

func aiQueryListenPort() int { return aiQueryPort }
