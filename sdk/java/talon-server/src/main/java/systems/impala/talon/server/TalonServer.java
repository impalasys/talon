package systems.impala.talon.server;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class TalonServer implements AutoCloseable {
    private final Process process;
    private final Path tempDir;
    private final Path configPath;
    private final int grpcPort;
    private final int uiPort;
    private final boolean keepTempDir;
    private final ByteArrayOutputStream logs = new ByteArrayOutputStream();

    private TalonServer(Process process, Path tempDir, Path configPath, int grpcPort, int uiPort, boolean keepTempDir) {
        this.process = process;
        this.tempDir = tempDir;
        this.configPath = configPath;
        this.grpcPort = grpcPort;
        this.uiPort = uiPort;
        this.keepTempDir = keepTempDir;
    }

    public static TalonServer start() throws IOException, InterruptedException {
        return start(Options.defaults());
    }

    public static TalonServer start(Options options) throws IOException, InterruptedException {
        if (options.configPath() != null && (options.config() != null || options.dataDir() != null || options.provider() != null)) {
            throw new IllegalArgumentException(
                "configPath cannot be combined with config, dataDir, or provider; put those settings in the config file"
            );
        }
        if (options.config() != null && options.provider() != null) {
            throw new IllegalArgumentException("config cannot be combined with provider; put providers in the config object");
        }
        Path node = resolveTalonNode(options.talonNodePath());
        int grpcPort = options.grpcPort() == null ? freePort() : options.grpcPort();
        int uiPort = options.uiPort() == null ? freePort() : options.uiPort();
        Path tempDir = Files.createTempDirectory("talon-server-");
        Path configPath;
        if (options.configPath() != null) {
            configPath = options.configPath().toAbsolutePath().normalize();
        } else {
            Path dataDir = options.dataDir() == null ? null : options.dataDir().toAbsolutePath().normalize();
            Map<String, Object> config = options.config() == null
                ? defaultConfig(options.provider(), dataDir == null ? tempDir.resolve("data") : dataDir)
                : configWithDataDir(options.config(), dataDir);
            Path configDataDir = controlPlaneDataDir(config);
            if (configDataDir != null) {
                if (!configDataDir.isAbsolute()) {
                    configDataDir = tempDir.resolve(configDataDir);
                }
                Files.createDirectories(configDataDir);
            }
            configPath = tempDir.resolve("talon.json");
            Files.writeString(configPath, toJson(config) + "\n", StandardCharsets.UTF_8);
        }

        List<String> command = new ArrayList<>();
        command.add(node.toString());
        ProcessBuilder builder = new ProcessBuilder(command);
        Map<String, String> env = builder.environment();
        env.put("GRPC_ADDR", "127.0.0.1:" + grpcPort);
        env.put("GATEWAY_UI_ADDR", "127.0.0.1:" + uiPort);
        env.put("TALON_CONFIG_PATH", configPath.toString());
        env.put("RUST_LOG", "info");
        if (options.jwtSecret() != null && !options.jwtSecret().isEmpty()) {
            env.put("GATEWAY_JWT_SECRET", options.jwtSecret());
        }
        env.putAll(options.env());
        Process process = builder.redirectErrorStream(true).start();

        TalonServer server = new TalonServer(process, tempDir, configPath, grpcPort, uiPort, options.keepTempDir());
        server.captureLogs(process.getInputStream());
        try {
            server.waitForPort(options.startupTimeout());
        } catch (IOException | InterruptedException e) {
            server.close();
            throw e;
        }
        return server;
    }

    public String grpcEndpoint() {
        return "127.0.0.1:" + grpcPort;
    }

    public String uiEndpoint() {
        return "http://127.0.0.1:" + uiPort;
    }

    public Path tempDir() {
        return tempDir;
    }

    public Path configPath() {
        return configPath;
    }

    public synchronized String logs() {
        return logs.toString(StandardCharsets.UTF_8);
    }

    @Override
    public void close() throws IOException {
        process.destroy();
        try {
            if (!process.waitFor(2, java.util.concurrent.TimeUnit.SECONDS)) {
                process.destroyForcibly();
                process.waitFor();
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        if (!keepTempDir) {
            deleteRecursively(tempDir);
        }
    }

    private static Path resolveTalonNode(Path explicit) throws IOException {
        if (explicit != null) return explicit;
        String env = System.getenv("TALON_NODE_PATH");
        if (env != null && !env.isBlank()) return Path.of(env);
        String platform = platformName();
        Path bundled = extractBundledBinary(platform);
        if (bundled != null) return bundled;
        throw new IOException("talon-node binary not found; set TALON_NODE_PATH or bundle " + platform);
    }

    private static String platformName() throws IOException {
        String os = System.getProperty("os.name").toLowerCase();
        String arch = System.getProperty("os.arch").toLowerCase();
        if (os.contains("linux") && (arch.equals("amd64") || arch.equals("x86_64"))) return "linux-x64";
        if (os.contains("mac") && (arch.equals("aarch64") || arch.equals("arm64"))) return "darwin-arm64";
        throw new IOException("unsupported talon-node platform: " + os + "-" + arch);
    }

    private static Path extractBundledBinary(String platform) throws IOException {
        String resource = "/talon/bin/" + platform + "/talon-node";
        try (InputStream stream = TalonServer.class.getResourceAsStream(resource)) {
            if (stream == null) return null;
            Path dir = Files.createTempDirectory("talon-node-");
            dir.toFile().deleteOnExit();
            Path target = dir.resolve("talon-node");
            Files.copy(stream, target);
            target.toFile().setExecutable(true);
            target.toFile().deleteOnExit();
            return target;
        }
    }

    private void waitForPort(Duration timeout) throws IOException, InterruptedException {
        Instant deadline = Instant.now().plus(timeout);
        while (Instant.now().isBefore(deadline)) {
            try (Socket ignored = new Socket("127.0.0.1", grpcPort)) {
                return;
            } catch (IOException ignored) {
                Thread.sleep(100);
            }
        }
        throw new IOException("timeout waiting for Talon gRPC endpoint; logs:\n" + logs());
    }

    private void captureLogs(InputStream stream) {
        Thread thread = new Thread(() -> {
            byte[] buffer = new byte[8192];
            int read;
            try {
                while ((read = stream.read(buffer)) >= 0) {
                    synchronized (this) {
                        logs.write(buffer, 0, read);
                    }
                }
            } catch (IOException ignored) {
            }
        });
        thread.setDaemon(true);
        thread.start();
    }

    private static int freePort() throws IOException {
        try (ServerSocket socket = new ServerSocket(0)) {
            socket.setReuseAddress(true);
            return socket.getLocalPort();
        }
    }

    static Map<String, Object> defaultConfig(Provider provider, Path dataDir) {
        Map<String, Object> database = new LinkedHashMap<>();
        database.put("driver", "sqlite");
        database.put("data_dir", dataDir.toString());
        Map<String, Object> messageBroker = new LinkedHashMap<>();
        messageBroker.put("driver", "local_socket");
        Map<String, Object> controlPlane = new LinkedHashMap<>();
        controlPlane.put("database", database);
        controlPlane.put("message_broker", messageBroker);
        Map<String, Object> config = new LinkedHashMap<>();
        config.put("control_plane", controlPlane);
        if (provider != null) {
            String name = provider.name() == null || provider.name().isBlank() ? "mock" : provider.name();
            Map<String, Object> providerConfig = new LinkedHashMap<>();
            providerConfig.put("type", "openai_compatible");
            providerConfig.put("base_url", provider.baseUrl());
            providerConfig.put("model", provider.model());
            providerConfig.put("api_key", provider.apiKey());
            config.put("providers", Map.of(name, providerConfig));
            config.put("default_provider", name);
        }
        return config;
    }

    static Map<String, Object> configWithDataDir(Map<String, Object> config, Path dataDir) {
        Map<String, Object> copy = deepCopyMap(config);
        if (dataDir == null) return copy;
        Map<String, Object> controlPlane = ensureMap(copy, "control_plane");
        Map<String, Object> database = ensureMap(controlPlane, "database");
        database.put("data_dir", dataDir.toString());
        return copy;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> ensureMap(Map<String, Object> target, String key) {
        Object current = target.get(key);
        if (current instanceof Map<?, ?> currentMap) {
            return (Map<String, Object>) currentMap;
        }
        Map<String, Object> value = new LinkedHashMap<>();
        target.put(key, value);
        return value;
    }

    private static Path controlPlaneDataDir(Map<String, Object> config) {
        Object controlPlane = config.get("control_plane");
        if (!(controlPlane instanceof Map<?, ?> controlPlaneMap)) return null;
        Object database = controlPlaneMap.get("database");
        if (!(database instanceof Map<?, ?> databaseMap)) return null;
        Object dataDir = databaseMap.get("data_dir");
        if (!(dataDir instanceof String value) || value.isBlank()) return null;
        return Path.of(value);
    }

    private static Map<String, Object> deepCopyMap(Map<String, Object> input) {
        Map<String, Object> copy = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : input.entrySet()) {
            copy.put(entry.getKey(), deepCopyValue(entry.getValue()));
        }
        return copy;
    }

    @SuppressWarnings("unchecked")
    private static Object deepCopyValue(Object value) {
        if (value instanceof Map<?, ?> map) {
            Map<String, Object> copy = new LinkedHashMap<>();
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                copy.put(String.valueOf(entry.getKey()), deepCopyValue(entry.getValue()));
            }
            return copy;
        }
        if (value instanceof List<?> list) {
            return list.stream().map(TalonServer::deepCopyValue).toList();
        }
        return value;
    }

    private static String toJson(Object value) {
        if (value == null) return "null";
        if (value instanceof String string) return jsonQuoted(string);
        if (value instanceof Number || value instanceof Boolean) return value.toString();
        if (value instanceof Map<?, ?> map) {
            StringBuilder json = new StringBuilder("{");
            boolean first = true;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!first) json.append(",");
                first = false;
                json.append(jsonQuoted(String.valueOf(entry.getKey()))).append(":").append(toJson(entry.getValue()));
            }
            return json.append("}").toString();
        }
        if (value instanceof Collection<?> collection) {
            StringBuilder json = new StringBuilder("[");
            boolean first = true;
            for (Object entry : collection) {
                if (!first) json.append(",");
                first = false;
                json.append(toJson(entry));
            }
            return json.append("]").toString();
        }
        return jsonQuoted(String.valueOf(value));
    }

    private static String jsonQuoted(String value) {
        StringBuilder quoted = new StringBuilder("\"");
        for (int index = 0; index < value.length(); index++) {
            char ch = value.charAt(index);
            switch (ch) {
                case '\\' -> quoted.append("\\\\");
                case '"' -> quoted.append("\\\"");
                case '\n' -> quoted.append("\\n");
                case '\r' -> quoted.append("\\r");
                case '\t' -> quoted.append("\\t");
                default -> quoted.append(ch);
            }
        }
        return quoted.append("\"").toString();
    }

    private static void deleteRecursively(Path path) throws IOException {
        if (!Files.exists(path)) return;
        try (var walk = Files.walk(path)) {
            for (Path entry : walk.sorted((a, b) -> b.compareTo(a)).toList()) {
                Files.deleteIfExists(entry);
            }
        }
    }
}
