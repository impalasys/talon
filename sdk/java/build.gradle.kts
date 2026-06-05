import org.gradle.api.publish.PublishingExtension
import org.gradle.api.publish.maven.MavenPublication

plugins {
    `java-library`
}

subprojects {
    apply(plugin = "java-library")
    apply(plugin = "maven-publish")

    group = "systems.impala"
    version = "0.1.15"

    java {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    tasks.withType<Test>().configureEach {
        useJUnitPlatform()
    }

    extensions.configure<PublishingExtension>("publishing") {
        publications {
            create<MavenPublication>("mavenJava") {
                from(components["java"])
                pom {
                    name.set("Talon ${project.name}")
                    description.set("Talon SDK package ${project.name}")
                    url.set("https://github.com/impalasys/talon")
                    licenses {
                        license {
                            name.set("AGPL-3.0-only")
                            url.set("https://www.gnu.org/licenses/agpl-3.0.en.html")
                        }
                    }
                    scm {
                        url.set("https://github.com/impalasys/talon")
                        connection.set("scm:git:https://github.com/impalasys/talon.git")
                        developerConnection.set("scm:git:ssh://git@github.com/impalasys/talon.git")
                    }
                }
            }
        }
        repositories {
            maven {
                name = "release"
                url = uri(System.getenv("MAVEN_PUBLISH_URL") ?: layout.buildDirectory.dir("repo"))
                credentials {
                    username = System.getenv("MAVEN_PUBLISH_USERNAME") ?: ""
                    password = System.getenv("MAVEN_PUBLISH_PASSWORD") ?: ""
                }
            }
        }
    }
}
