plugins {
    application
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

application {
    mainClass.set("systems.impala.talon.examples.App")
}

dependencies {
    implementation("systems.impala:talon-client:0.1.0")
    implementation("systems.impala:talon-server:0.1.0")
    implementation("io.grpc:grpc-netty-shaded:1.76.0")
}

