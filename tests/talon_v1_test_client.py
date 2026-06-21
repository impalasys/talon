from proto.talon.v1.api_pb2_grpc import (
    ChannelServiceStub,
    KnowledgeServiceStub,
    NamespaceServiceStub,
    ResourceServiceStub,
    SessionServiceStub,
    WorkflowServiceStub,
)


class TalonV1TestClient:
    def __init__(self, channel):
        self.namespaces = NamespaceServiceStub(channel)
        self.resources = ResourceServiceStub(channel)
        self.sessions = SessionServiceStub(channel)
        self.channels = ChannelServiceStub(channel)
        self.workflows = WorkflowServiceStub(channel)
        self.knowledge = KnowledgeServiceStub(channel)

    def CreateNamespace(self, request, *args, **kwargs):
        return self.namespaces.Create(request, *args, **kwargs)

    def GetNamespace(self, request, *args, **kwargs):
        return self.namespaces.Get(request, *args, **kwargs)

    def DeleteNamespace(self, request, *args, **kwargs):
        return self.namespaces.Delete(request, *args, **kwargs)

    def ListNamespaces(self, request, *args, **kwargs):
        return self.namespaces.List(request, *args, **kwargs)

    def CreateResource(self, request, *args, **kwargs):
        return self.resources.Create(request, *args, **kwargs)

    def GetResource(self, request, *args, **kwargs):
        return self.resources.Get(request, *args, **kwargs)

    def ListResources(self, request, *args, **kwargs):
        return self.resources.List(request, *args, **kwargs)

    def DeleteResource(self, request, *args, **kwargs):
        return self.resources.Delete(request, *args, **kwargs)

    def CreateSession(self, request, *args, **kwargs):
        return self.sessions.Create(request, *args, **kwargs)

    def GetSession(self, request, *args, **kwargs):
        return self.sessions.Get(request, *args, **kwargs)

    def ListSessions(self, request, *args, **kwargs):
        return self.sessions.List(request, *args, **kwargs)

    def ListSessionMessages(self, request, *args, **kwargs):
        return self.sessions.ListMessages(request, *args, **kwargs)

    def DeleteSession(self, request, *args, **kwargs):
        return self.sessions.Delete(request, *args, **kwargs)

    def ClearSession(self, request, *args, **kwargs):
        return self.sessions.Clear(request, *args, **kwargs)

    def SendMessage(self, request, *args, **kwargs):
        return self.sessions.SendMessage(request, *args, **kwargs)

    def StreamSessionParts(self, request, *args, **kwargs):
        return self.sessions.StreamParts(request, *args, **kwargs)

    def PostChannelMessage(self, request, *args, **kwargs):
        return self.channels.PostMessage(request, *args, **kwargs)

    def GetChannelMessage(self, request, *args, **kwargs):
        return self.channels.GetMessage(request, *args, **kwargs)

    def ListChannelMessages(self, request, *args, **kwargs):
        return self.channels.ListMessages(request, *args, **kwargs)

    def GetKnowledge(self, request, *args, **kwargs):
        return self.knowledge.Get(request, *args, **kwargs)

    def SearchKnowledge(self, request, *args, **kwargs):
        return self.knowledge.Search(request, *args, **kwargs)

    def CreateWorkflowRun(self, request, *args, **kwargs):
        return self.workflows.CreateRun(request, *args, **kwargs)

    def GetWorkflowRun(self, request, *args, **kwargs):
        return self.workflows.GetRun(request, *args, **kwargs)

    def ListWorkflowRuns(self, request, *args, **kwargs):
        return self.workflows.ListRuns(request, *args, **kwargs)

    def StreamWorkflowEvents(self, request, *args, **kwargs):
        return self.workflows.StreamEvents(request, *args, **kwargs)
