package systems.impala.talon.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Base64;
import org.junit.jupiter.api.Test;

final class TalonJwtTest {
    @Test
    void mintsScopedTalonJwt() {
        String token = TalonJwt.mint(
            "secret",
            new JwtOptions("browser-demo", Duration.ofMinutes(1), "demo", "copilot", null, "chat")
        );
        String[] segments = token.split("\\.");
        assertEquals(3, segments.length);

        String header = decode(segments[0]);
        String payload = decode(segments[1]);
        assertTrue(header.contains("\"alg\":\"HS256\""));
        assertTrue(header.contains("\"typ\":\"JWT\""));
        assertTrue(payload.contains("\"sub\":\"browser-demo\""));
        assertTrue(payload.contains("\"aud\":\"talon\""));
        assertTrue(payload.contains("\"talon:ns\":\"demo\""));
        assertTrue(payload.contains("\"talon:agent\":\"copilot\""));
        assertTrue(payload.contains("\"talon:channel\":\"chat\""));
        assertEquals("Bearer " + token, TalonJwt.authorizationHeader(token));
    }

    @Test
    void requiresNamespaceForChannelScope() {
        assertThrows(
            IllegalArgumentException.class,
            () -> TalonJwt.mint("secret", new JwtOptions("browser-demo", Duration.ofMinutes(1), null, null, null, "chat"))
        );
    }

    private static String decode(String segment) {
        return new String(Base64.getUrlDecoder().decode(segment), StandardCharsets.UTF_8);
    }
}
