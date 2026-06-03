plugins {
    `java-library`
}

dependencies {
    api("com.google.protobuf:protobuf-java:4.34.1")
    api("io.grpc:grpc-api:1.76.0")
    api("io.grpc:grpc-stub:1.76.0")
    api("io.grpc:grpc-protobuf:1.76.0")
    compileOnly("org.apache.tomcat:annotations-api:6.0.53")
    testImplementation("org.junit.jupiter:junit-jupiter:5.13.4")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher:1.13.4")
}
