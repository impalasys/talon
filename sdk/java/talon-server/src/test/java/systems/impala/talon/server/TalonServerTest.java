package systems.impala.talon.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.nio.file.Path;
import java.time.Duration;
import java.util.Map;
import org.junit.jupiter.api.Test;

final class TalonServerTest {
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
                new Options(null, Path.of("talon.yaml"), Map.<String, Object>of("workspace_dir", "."), null, null, null, false, Map.of(), Duration.ofSeconds(30), null)
            )
        );
    }
}
