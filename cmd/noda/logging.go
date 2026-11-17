package main

import (
	"fmt"
	"log/slog"
)

// parseLogLevel reads NODA_LOG_LEVEL. Empty means info.
func parseLogLevel(text string) (slog.Level, error) {
	switch text {
	case "", "info":
		return slog.LevelInfo, nil
	case "debug":
		return slog.LevelDebug, nil
	case "warn":
		return slog.LevelWarn, nil
	case "error":
		return slog.LevelError, nil
	}
	return 0, fmt.Errorf("unknown log level %q (want debug, info, warn or error)", text)
}
