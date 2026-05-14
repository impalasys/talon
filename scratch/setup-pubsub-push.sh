#!/bin/bash

export PUBSUB_EMULATOR_HOST=localhost:8085
export GCP_PROJECT_ID=talon-local
SESSION_DISPATCH_TOPIC=talon.session.dispatch
RESOURCE_LIFECYCLE_TOPIC=talon.resource.lifecycle

echo "Configuring GCP PubSub Emulator Push Subscriptions..."

# 1. Create the topics via curl directly to avoid needing gcloud installed inside host
curl -s -X PUT http://${PUBSUB_EMULATOR_HOST}/v1/projects/${GCP_PROJECT_ID}/topics/${SESSION_DISPATCH_TOPIC} > /dev/null
curl -s -X PUT http://${PUBSUB_EMULATOR_HOST}/v1/projects/${GCP_PROJECT_ID}/topics/${RESOURCE_LIFECYCLE_TOPIC} > /dev/null

# 2. Create the push subscriptions
curl -s -X PUT http://${PUBSUB_EMULATOR_HOST}/v1/projects/${GCP_PROJECT_ID}/subscriptions/talon-session-dispatch-push-sub \
  -H "Content-Type: application/json" \
  -d '{
    "topic": "projects/'${GCP_PROJECT_ID}'/topics/'${SESSION_DISPATCH_TOPIC}'",
    "pushConfig": {
      "pushEndpoint": "http://worker:8081/pubsub/push"
    }
  }' > /dev/null

curl -s -X PUT http://${PUBSUB_EMULATOR_HOST}/v1/projects/${GCP_PROJECT_ID}/subscriptions/talon-resource-lifecycle-push-sub \
  -H "Content-Type: application/json" \
  -d '{
    "topic": "projects/'${GCP_PROJECT_ID}'/topics/'${RESOURCE_LIFECYCLE_TOPIC}'",
    "pushConfig": {
      "pushEndpoint": "http://worker:8081/pubsub/push"
    }
  }' > /dev/null

echo "PubSub Emulator configured successfully! Split topics now forward to worker:8081."
