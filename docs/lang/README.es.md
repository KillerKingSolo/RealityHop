# Reality Hop (Español)

[English](../../README.md) · **Español** · [简体中文](README.zh.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [فارسی](README.fa.md) · [العربية](README.ar.md) · [Türkçe](README.tr.md) · [Tiếng Việt](README.vi.md)

> Traducción automática del README en inglés. Si algo no está claro, la versión en inglés es la referencia. Las correcciones son bienvenidas en un issue de GitHub.

**Haz que tu VPN funcione en redes que bloquean las VPN.** Reality Hop envuelve la conexión de tu VPN en un túnel VLESS + REALITY ([sing-box](https://github.com/SagerNet/sing-box)). Para la red parece HTTPS normal hacia una web grande, como cloudflare.com. Está hecho para:

- Wi-Fi de escuela, trabajo, hotel y residencia
- Wi-Fi pública en aeropuertos, cafeterías, trenes y bibliotecas
- routers con «bloqueo de VPN» y proveedores que frenan o bloquean el tráfico VPN
- países que filtran internet, como la Gran Muralla de China, Rusia, Irán, Turkmenistán y los EAU

Toma nodos públicos gratuitos de [rtwo2/FastNodes](https://github.com/rtwo2/FastNodes), los prueba él mismo, usa los más rápidos que funcionan y sustituye los caídos por sí solo.

![Asistente de configuración](../images/setup-welcome.png)

## Cómo funciona

App VPN → Reality Hop en tu PC (SOCKS5 `127.0.0.1:2080`) → nodo REALITY público (parece HTTPS a cloudflare.com) → servidor VPN → internet. El cifrado de tu VPN sigue intacto dentro durante todo el camino.

## Instalar

1. Descarga `RealityHop.exe` de la [última versión](https://github.com/KillerKingSolo/RealityHop/releases/latest). Requiere Windows 10 u 11 de 64 bits.
2. Ejecútalo y elige tu idioma.
3. El asistente instala Reality Hop (un aviso de UAC), descarga sing-box de sus versiones oficiales (comprueba el SHA-256), encuentra los primeros nodos e inicia el túnel. Alrededor de un minuto.

## Idioma

Reality Hop pregunta el idioma en el primer inicio. Cámbialo después en **Ajustes → Idioma**. El registro de actividad y los informes de problemas siguen en inglés, para que el soporte pueda leerlos.

## Configurar la app VPN

Apunta tu app VPN al proxy SOCKS5 `127.0.0.1`, puerto `2080`, sin usuario ni contraseña. Usa **OpenVPN sobre TCP, puerto 443**: un proxy SOCKS5 solo lleva TCP.

**Private Internet Access (PIA):**

1. PIA → **Settings → Protocols**: Protocol **OpenVPN**, Transport **TCP**, Remote Port **443**.
2. **Settings → Multi-Hop**: marca **Multi-hop and Obfuscation** y elige **SOCKS5 Proxy**.
3. Pulsa **EDIT**, host `127.0.0.1`, puerto `2080`, usuario y contraseña vacíos, y guarda.
4. Solo si Split Tunnel tiene **All Other Apps** en **Use VPN**: **Settings → Split Tunnel**, **Add Application**, elige `C:\Program Files\RealityHop\PC\rh-core.exe` y ponlo en **Bypass VPN**. Con All Other Apps en Bypass VPN no hace falta esta regla.
5. Conecta.

![PIA Split Tunnel con rh-core.exe en Bypass VPN](../images/pia-split-tunnel.png)

**Otras apps VPN:** cualquier app con ajuste de proxy SOCKS5 funciona igual. En un archivo de OpenVPN: `proto tcp`, el servidor en el puerto 443 y la línea `socks-proxy 127.0.0.1 2080`. Detalles en inglés: [VPN app setup](../vpn-setup.md).

## Router GL.iNet (opcional)

Con un router GL.iNet (firmware 4.x), los dispositivos que elijas (móvil, consola, TV) también pasan por el túnel, sin instalar nada en ellos. El modo **Solo router** no ejecuta nada en el PC: el router lleva por el túnel los dispositivos elegidos o toda la red. Guía en inglés: [router setup](../router-setup.md).

## Uso diario

- **Icono de la bandeja:** el centro es el estado general; el anillo tiene un segmento por nodo (verde funciona, ámbar lento, rojo caído).
- **Comprobar ahora**, **Reiniciar túnel**, **Mostrar candidatos** (probar nodos y ponerlos en ranuras), **Rescatar** cuando todos los nodos están caídos.
- **Detener / Iniciar** para cuando viajas. Mientras esté detenido, desactiva el proxy de la app VPN.
- Reality Hop se actualiza solo desde las versiones de GitHub (con comprobación SHA-256).

## Privacidad y seguridad

- **Los nodos públicos los gestionan desconocidos.** Tu VPN sigue cifrándolo todo de extremo a extremo, así que un nodo solo ve tráfico VPN cifrado. Mantén siempre la VPN encima.
- Reality Hop contacta GitHub (lista de nodos, sing-box, versiones de Reality Hop), y a través del túnel `api.ipify.org`, `www.gstatic.com/generate_204`, `ip-api.com` y el DNS de Quad9. No envía nada sobre ti y no tiene telemetría.
- Asegúrate de que puedes usar una VPN en tu red y en tu país. Reality Hop no cambia la ley donde estás.

## Problemas

Usa **Informar de un problema** en la ventana: crea un informe anonimizado para adjuntar a un issue de GitHub. Nunca adjuntes `pc-config.json`, `router-config.json` ni `vault.json`. Guía en inglés: [troubleshooting](../troubleshooting.md).

## Licencia

[CC BY-NC-ND 4.0](../../LICENSE): compártelo sin cambios y con crédito, sin versiones modificadas y sin uso comercial.
