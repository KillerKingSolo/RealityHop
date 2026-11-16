package box

// Reality Hop router build: no debug HTTP server (pprof).
import (
	"net/http"

	"github.com/sagernet/sing-box/option"
	E "github.com/sagernet/sing/common/exceptions"
)

func startDebugHTTPServer(options option.DebugOptions) (*http.Server, error) {
	if options.Listen == "" {
		return nil, nil
	}
	return nil, E.New("debug server not in this build")
}
