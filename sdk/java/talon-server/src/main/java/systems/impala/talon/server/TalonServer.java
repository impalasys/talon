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
        Path node = resolveTalonNode(options.talonNodePath());
        int grpcPort = options.grpcPort() == null ? freePort() : options.grpcPort();
        int uiPort = options.uiPort() == null ? freePort() : options.uiPort();
        Path tempDir = Files.createTempDirectory("talon-server-");
        Files.createDirectories(tempDir.resolve("data"));
        Path configPath = tempDir.resolve("talon.yaml");
        Files.writeString(configPath, configYaml(options.provider()), StandardCharsets.UTF_8);

        List<String> command = new ArrayList<>();
        command.add(node.toString());
        ProcessBuilder builder = new ProcessBuilder(command);
        Map<String, String> env = builder.environment();
        env.put("GRPC_ADDR", "127.0.0.1:" + grpcPort);
        env.put("GATEWAY_UI_ADDR", "127.0.0.1:" + uiPort);
        env.put("TALON_CONFIG_PATH", configPath.toString());
        env.put("RUST_LOG", "info");
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

    private static String configYaml(Provider provider) {
        StringBuilder yaml = new StringBuilder();
        if (provider != null) {
            String name = provider.name() == null || provider.name().isBlank() ? "mock" : provider.name();
            yaml.append("providers:\n  ").append(name).append(":\n")
                .append("    type: openai_compatible\n")
                .append("    base_url: \"").append(provider.baseUrl()).append("\"\n")
                .append("    model: \"").append(provider.model()).append("\"\n")
                .append("    api_key: \"").append(provider.apiKey()).append("\"\n")
                .append("default_provider: \"").append(name).append("\"\n");
        }
        yaml.append("control_plane:\n")
            .append("  database:\n")
            .append("    driver: sqlite\n")
            .append("    data_dir: ./data\n")
            .append("  message_broker:\n")
            .append("    driver: local_socket\n");
        return yaml.toString();
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
