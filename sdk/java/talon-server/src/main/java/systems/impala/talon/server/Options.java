package systems.impala.talon.server;

import java.nio.file.Path;
import java.time.Duration;
import java.util.Map;

public record Options(
    Path talonNodePath,
    Integer grpcPort,
    Integer uiPort,
    boolean keepTempDir,
    Map<String, String> env,
    Duration startupTimeout,
    Provider provider,
    String jwtSecret
) {
    public static Options defaults() {
        return new Options(null, null, null, false, Map.of(), Duration.ofSeconds(30), null, null);
    }
}
