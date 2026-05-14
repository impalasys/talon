"""Client stubs for the Talon gateway test proto fixture.

This file is intentionally checked in for the Python chat tests and only
implements the client-side stub methods those tests can import. The server-side
helpers from grpcio-tools are not needed in this repo.
"""

import grpc

from proto import events_pb2 as proto_dot_events__pb2
from proto import gateway_pb2 as proto_dot_gateway__pb2


class GatewayServiceStub:
    def __init__(self, channel):
        self.CreateAgent = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateAgent",
            request_serializer=proto_dot_gateway__pb2.CreateAgentRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.AgentResponse.FromString,
        )
        self.GetAgent = channel.unary_unary(
            "/talon.gateway.GatewayService/GetAgent",
            request_serializer=proto_dot_gateway__pb2.GetAgentRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.GetAgentResponse.FromString,
        )
        self.ModifyAgent = channel.unary_unary(
            "/talon.gateway.GatewayService/ModifyAgent",
            request_serializer=proto_dot_gateway__pb2.ModifyAgentRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.AgentResponse.FromString,
        )
        self.ListAgents = channel.unary_unary(
            "/talon.gateway.GatewayService/ListAgents",
            request_serializer=proto_dot_gateway__pb2.ListAgentsRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListAgentsResponse.FromString,
        )
        self.GetKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/GetKnowledge",
            request_serializer=proto_dot_gateway__pb2.GetKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.KnowledgeResponse.FromString,
        )
        self.SearchKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/SearchKnowledge",
            request_serializer=proto_dot_gateway__pb2.SearchKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.SearchKnowledgeResponse.FromString,
        )
        self.CreateNamespaceKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateNamespaceKnowledge",
            request_serializer=proto_dot_gateway__pb2.CreateNamespaceKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.NamespaceKnowledgeResponse.FromString,
        )
        self.GetNamespaceKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/GetNamespaceKnowledge",
            request_serializer=proto_dot_gateway__pb2.GetNamespaceKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.NamespaceKnowledgeResponse.FromString,
        )
        self.ListNamespaceKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/ListNamespaceKnowledge",
            request_serializer=proto_dot_gateway__pb2.ListNamespaceKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListNamespaceKnowledgeResponse.FromString,
        )
        self.DeleteNamespaceKnowledge = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteNamespaceKnowledge",
            request_serializer=proto_dot_gateway__pb2.DeleteNamespaceKnowledgeRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteNamespaceKnowledgeResponse.FromString,
        )
        self.CreateSession = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateSession",
            request_serializer=proto_dot_gateway__pb2.CreateSessionRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.SessionResponse.FromString,
        )
        self.GetSession = channel.unary_unary(
            "/talon.gateway.GatewayService/GetSession",
            request_serializer=proto_dot_gateway__pb2.GetSessionRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.SessionResponse.FromString,
        )
        self.ListSessions = channel.unary_unary(
            "/talon.gateway.GatewayService/ListSessions",
            request_serializer=proto_dot_gateway__pb2.ListSessionsRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListSessionsResponse.FromString,
        )
        self.DeleteSession = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteSession",
            request_serializer=proto_dot_gateway__pb2.DeleteSessionRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteSessionResponse.FromString,
        )
        self.SendMessage = channel.unary_unary(
            "/talon.gateway.GatewayService/SendMessage",
            request_serializer=proto_dot_gateway__pb2.SendMessageRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.SendMessageResponse.FromString,
        )
        self.StreamSessionSteps = channel.unary_stream(
            "/talon.gateway.GatewayService/StreamSessionSteps",
            request_serializer=proto_dot_gateway__pb2.StreamSessionStepsRequest.SerializeToString,
            response_deserializer=proto_dot_events__pb2.SessionStepEvent.FromString,
        )
        self.CreateSchedule = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateSchedule",
            request_serializer=proto_dot_gateway__pb2.CreateScheduleRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ScheduleResponse.FromString,
        )
        self.GetSchedule = channel.unary_unary(
            "/talon.gateway.GatewayService/GetSchedule",
            request_serializer=proto_dot_gateway__pb2.GetScheduleRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ScheduleResponse.FromString,
        )
        self.ModifySchedule = channel.unary_unary(
            "/talon.gateway.GatewayService/ModifySchedule",
            request_serializer=proto_dot_gateway__pb2.ModifyScheduleRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ScheduleResponse.FromString,
        )
        self.ListSchedules = channel.unary_unary(
            "/talon.gateway.GatewayService/ListSchedules",
            request_serializer=proto_dot_gateway__pb2.ListSchedulesRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListSchedulesResponse.FromString,
        )
        self.DeleteSchedule = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteSchedule",
            request_serializer=proto_dot_gateway__pb2.DeleteScheduleRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteScheduleResponse.FromString,
        )
        self.CreateNamespace = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateNamespace",
            request_serializer=proto_dot_gateway__pb2.CreateNamespaceRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.NamespaceResponse.FromString,
        )
        self.GetNamespace = channel.unary_unary(
            "/talon.gateway.GatewayService/GetNamespace",
            request_serializer=proto_dot_gateway__pb2.GetNamespaceRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.NamespaceResponse.FromString,
        )
        self.DeleteNamespace = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteNamespace",
            request_serializer=proto_dot_gateway__pb2.DeleteNamespaceRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.NamespaceResponse.FromString,
        )
        self.ListNamespaces = channel.unary_unary(
            "/talon.gateway.GatewayService/ListNamespaces",
            request_serializer=proto_dot_gateway__pb2.ListNamespacesRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListNamespacesResponse.FromString,
        )
        self.CreateAgentTemplate = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateAgentTemplate",
            request_serializer=proto_dot_gateway__pb2.CreateAgentTemplateRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.AgentTemplateResponse.FromString,
        )
        self.GetAgentTemplate = channel.unary_unary(
            "/talon.gateway.GatewayService/GetAgentTemplate",
            request_serializer=proto_dot_gateway__pb2.GetAgentTemplateRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.AgentTemplateResponse.FromString,
        )
        self.ListAgentTemplates = channel.unary_unary(
            "/talon.gateway.GatewayService/ListAgentTemplates",
            request_serializer=proto_dot_gateway__pb2.ListAgentTemplatesRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListAgentTemplatesResponse.FromString,
        )
        self.DeleteAgentTemplate = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteAgentTemplate",
            request_serializer=proto_dot_gateway__pb2.DeleteAgentTemplateRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteAgentTemplateResponse.FromString,
        )
        self.CreateMcpServer = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateMcpServer",
            request_serializer=proto_dot_gateway__pb2.CreateMcpServerRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.McpServerResponse.FromString,
        )
        self.GetMcpServer = channel.unary_unary(
            "/talon.gateway.GatewayService/GetMcpServer",
            request_serializer=proto_dot_gateway__pb2.GetMcpServerRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.McpServerResponse.FromString,
        )
        self.ListMcpServers = channel.unary_unary(
            "/talon.gateway.GatewayService/ListMcpServers",
            request_serializer=proto_dot_gateway__pb2.ListMcpServersRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListMcpServersResponse.FromString,
        )
        self.DeleteMcpServer = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteMcpServer",
            request_serializer=proto_dot_gateway__pb2.DeleteMcpServerRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteMcpServerResponse.FromString,
        )
        self.CreateMcpServerBinding = channel.unary_unary(
            "/talon.gateway.GatewayService/CreateMcpServerBinding",
            request_serializer=proto_dot_gateway__pb2.CreateMcpServerBindingRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.McpServerBindingResponse.FromString,
        )
        self.GetMcpServerBinding = channel.unary_unary(
            "/talon.gateway.GatewayService/GetMcpServerBinding",
            request_serializer=proto_dot_gateway__pb2.GetMcpServerBindingRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.McpServerBindingResponse.FromString,
        )
        self.ListMcpServerBindings = channel.unary_unary(
            "/talon.gateway.GatewayService/ListMcpServerBindings",
            request_serializer=proto_dot_gateway__pb2.ListMcpServerBindingsRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.ListMcpServerBindingsResponse.FromString,
        )
        self.DeleteMcpServerBinding = channel.unary_unary(
            "/talon.gateway.GatewayService/DeleteMcpServerBinding",
            request_serializer=proto_dot_gateway__pb2.DeleteMcpServerBindingRequest.SerializeToString,
            response_deserializer=proto_dot_gateway__pb2.DeleteMcpServerBindingResponse.FromString,
        )
