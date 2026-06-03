package systems.impala.talon.server;

import java.time.Duration;

public record JwtOptions(
    String subject,
    Duration ttl,
    String namespace,
    String agent,
    String session,
    String channel
) {
    public static JwtOptions defaults() {
        return new JwtOptions("talon-sdk", Duration.ofHours(1), null, null, null, null);
    }
}
