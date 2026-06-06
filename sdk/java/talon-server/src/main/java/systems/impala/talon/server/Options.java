package systems.impala.talon.server;

import java.nio.file.Path;
import java.time.Duration;
import java.util.Map;

public record Options(
    Path talonNodePath,
    Path configPath,
    Map<String, Object> config,
    Path dataDir,
    Integer grpcPort,
    Integer uiPort,
    boolean keepTempDir,
    Map<String, String> env,
    Duration startupTimeout,
    Provider provider,
    String jwtSecret
) {
    public static Options defaults() {
        return new Options(null, null, null, null, null, null, false, Map.of(), Duration.ofSeconds(30), null, null);
    }
}
