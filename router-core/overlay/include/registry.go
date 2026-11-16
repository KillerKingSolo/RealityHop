package include

// Reality Hop router build: only what Reality Hop uses on the router. In: tproxy (the device diversion), direct
// (the DNS inbound) and mixed (the node test). Out: direct, block, urltest, selector and VLESS. DNS: DoH, and local
// (the system resolver), which sing-box falls back to when a config has no DNS servers, as the node test's config.
import (
	"context"

	"github.com/sagernet/sing-box"
	"github.com/sagernet/sing-box/adapter/certificate"
	"github.com/sagernet/sing-box/adapter/endpoint"
	"github.com/sagernet/sing-box/adapter/inbound"
	"github.com/sagernet/sing-box/adapter/outbound"
	"github.com/sagernet/sing-box/adapter/service"
	"github.com/sagernet/sing-box/dns"
	"github.com/sagernet/sing-box/dns/transport"
	"github.com/sagernet/sing-box/dns/transport/local"
	"github.com/sagernet/sing-box/protocol/block"
	"github.com/sagernet/sing-box/protocol/direct"
	"github.com/sagernet/sing-box/protocol/group"
	"github.com/sagernet/sing-box/protocol/mixed"
	"github.com/sagernet/sing-box/protocol/redirect"
	"github.com/sagernet/sing-box/protocol/vless"
)

func Context(ctx context.Context) context.Context {
	return box.Context(ctx, InboundRegistry(), OutboundRegistry(), EndpointRegistry(), DNSTransportRegistry(), ServiceRegistry(), CertificateProviderRegistry())
}

func InboundRegistry() *inbound.Registry {
	registry := inbound.NewRegistry()
	redirect.RegisterTProxy(registry)
	direct.RegisterInbound(registry)
	mixed.RegisterInbound(registry)
	return registry
}

func OutboundRegistry() *outbound.Registry {
	registry := outbound.NewRegistry()
	direct.RegisterOutbound(registry)
	block.RegisterOutbound(registry)
	group.RegisterSelector(registry)
	group.RegisterURLTest(registry)
	vless.RegisterOutbound(registry)
	return registry
}

func EndpointRegistry() *endpoint.Registry { return endpoint.NewRegistry() }

func DNSTransportRegistry() *dns.TransportRegistry {
	registry := dns.NewTransportRegistry()
	transport.RegisterHTTPS(registry)
	local.RegisterTransport(registry)
	return registry
}

func ServiceRegistry() *service.Registry { return service.NewRegistry() }

func CertificateProviderRegistry() *certificate.Registry { return certificate.NewRegistry() }
