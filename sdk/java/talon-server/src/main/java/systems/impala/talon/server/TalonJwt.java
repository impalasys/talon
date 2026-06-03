package systems.impala.talon.server;

import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.Map;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

public final class TalonJwt {
    private TalonJwt() {}

    public static String mint(String secret, JwtOptions options) {
        if (secret == null || secret.isEmpty()) {
            throw new IllegalArgumentException("secret is required");
        }
        JwtOptions effective = options == null ? JwtOptions.defaults() : options;
        String subject = effective.subject() == null ? "talon-sdk" : effective.subject();
        if (subject.isBlank()) {
            throw new IllegalArgumentException("subject is required");
        }
        var ttl = effective.ttl() == null ? JwtOptions.defaults().ttl() : effective.ttl();
        if (ttl.isZero() || ttl.isNegative()) {
            throw new IllegalArgumentException("ttl must be positive");
        }
        if (effective.channel() != null && effective.namespace() == null) {
            throw new IllegalArgumentException("channel-scoped JWTs require namespace");
        }

        Map<String, Object> claims = new LinkedHashMap<>();
        claims.put("sub", subject);
        claims.put("aud", "talon");
        claims.put("exp", Instant.now().plus(ttl).getEpochSecond());
        putOptional(claims, "talon:ns", effective.namespace());
        putOptional(claims, "talon:agent", effective.agent());
        putOptional(claims, "talon:session", effective.session());
        putOptional(claims, "talon:channel", effective.channel());

        String header = segment(Map.of("alg", "HS256", "typ", "JWT"));
        String payload = segment(claims);
        String message = header + "." + payload;
        return message + "." + sign(secret, message);
    }

    public static String authorizationHeader(String token) {
        if (token == null || token.isBlank()) {
            throw new IllegalArgumentException("token is required");
        }
        return "Bearer " + token;
    }

    private static void putOptional(Map<String, Object> claims, String key, String value) {
        if (value == null) return;
        if (value.isBlank()) {
            throw new IllegalArgumentException(key + " must not be empty");
        }
        claims.put(key, value);
    }

    private static String segment(Map<String, ?> value) {
        return Base64.getUrlEncoder().withoutPadding().encodeToString(json(value).getBytes(StandardCharsets.UTF_8));
    }

    private static String sign(String secret, String message) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            return Base64.getUrlEncoder().withoutPadding().encodeToString(mac.doFinal(message.getBytes(StandardCharsets.UTF_8)));
        } catch (Exception e) {
            throw new IllegalStateException("failed to sign Talon JWT", e);
        }
    }

    private static String json(Map<String, ?> value) {
        StringBuilder out = new StringBuilder("{");
        boolean first = true;
        for (var entry : value.entrySet()) {
            if (!first) out.append(",");
            first = false;
            out.append(quote(entry.getKey())).append(":");
            Object entryValue = entry.getValue();
            if (entryValue instanceof Number || entryValue instanceof Boolean) {
                out.append(entryValue);
            } else {
                out.append(quote(String.valueOf(entryValue)));
            }
        }
        return out.append("}").toString();
    }

    private static String quote(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        out.append(String.format("\\u%04x", (int) ch));
                    } else {
                        out.append(ch);
                    }
                }
            }
        }
        return out.append("\"").toString();
    }
}
