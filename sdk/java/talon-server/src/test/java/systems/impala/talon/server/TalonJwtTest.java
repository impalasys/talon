package systems.impala.talon.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Base64;
import java.util.Map;
import org.junit.jupiter.api.Test;

final class TalonJwtTest {
    @Test
    void generatedConfigUsesRequestedDataDir() {
        Map<String, Object> config = TalonServer.defaultConfig(null, Path.of("/tmp/talon-data"));
        Map<?, ?> controlPlane = (Map<?, ?>) config.get("control_plane");
        Map<?, ?> database = (Map<?, ?>) controlPlane.get("database");
        Map<?, ?> messageBroker = (Map<?, ?>) controlPlane.get("message_broker");
        assertEquals("sqlite", database.get("driver"));
        assertEquals("/tmp/talon-data", database.get("data_dir"));
        assertEquals("local_socket", messageBroker.get("driver"));
    }

    @Test
    void configCanSpecifyGeneralTalonSettings() {
        Map<String, Object> config = TalonServer.configWithDataDir(
            Map.of(
                "workspace_dir", "/tmp/workspace",
                "default_provider", "openai",
                "control_plane", Map.of(
                    "database", Map.of("driver", "sqlite"),
                    "message_broker", Map.of("driver", "local_socket")
                )
            ),
            null
        );
        assertEquals("/tmp/workspace", config.get("workspace_dir"));
        assertEquals("openai", config.get("default_provider"));
    }

    @Test
    void jsonSerializerHandlesArraysAndControlCharacters() {
        assertEquals("[\"a\",\"b\"]", TalonServer.toJson(new String[] {"a", "b"}));
        assertEquals("[1,2,3]", TalonServer.toJson(new int[] {1, 2, 3}));
        assertEquals("\"line\\nback\\bform\\fzero\\u0000\"", TalonServer.toJson("line\nback\bform\fzero\u0000"));
    }

    @Test
    void rejectsAmbiguousConfigOptions() {
        assertThrows(
            IllegalArgumentException.class,
            () -> TalonServer.start(
                new Options(null, Path.of("talon.yaml"), Map.<String, Object>of("workspace_dir", "."), null, null, null, false, Map.of(), Duration.ofSeconds(30), null, null)
            )
        );
    }

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
