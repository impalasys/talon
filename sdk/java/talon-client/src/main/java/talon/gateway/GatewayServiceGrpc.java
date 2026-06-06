package talon.gateway;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class GatewayServiceGrpc {

  private GatewayServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.gateway.GatewayService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentRequest,
      talon.gateway.Gateway.AgentResponse> getCreateAgentMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateAgent",
      requestType = talon.gateway.Gateway.CreateAgentRequest.class,
      responseType = talon.gateway.Gateway.AgentResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentRequest,
      talon.gateway.Gateway.AgentResponse> getCreateAgentMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentRequest, talon.gateway.Gateway.AgentResponse> getCreateAgentMethod;
    if ((getCreateAgentMethod = GatewayServiceGrpc.getCreateAgentMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateAgentMethod = GatewayServiceGrpc.getCreateAgentMethod) == null) {
          GatewayServiceGrpc.getCreateAgentMethod = getCreateAgentMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateAgentRequest, talon.gateway.Gateway.AgentResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateAgent"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateAgentRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AgentResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateAgent"))
              .build();
        }
      }
    }
    return getCreateAgentMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentRequest,
      talon.gateway.Gateway.GetAgentResponse> getGetAgentMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetAgent",
      requestType = talon.gateway.Gateway.GetAgentRequest.class,
      responseType = talon.gateway.Gateway.GetAgentResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentRequest,
      talon.gateway.Gateway.GetAgentResponse> getGetAgentMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentRequest, talon.gateway.Gateway.GetAgentResponse> getGetAgentMethod;
    if ((getGetAgentMethod = GatewayServiceGrpc.getGetAgentMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetAgentMethod = GatewayServiceGrpc.getGetAgentMethod) == null) {
          GatewayServiceGrpc.getGetAgentMethod = getGetAgentMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetAgentRequest, talon.gateway.Gateway.GetAgentResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetAgent"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetAgentRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetAgentResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetAgent"))
              .build();
        }
      }
    }
    return getGetAgentMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyAgentRequest,
      talon.gateway.Gateway.AgentResponse> getModifyAgentMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ModifyAgent",
      requestType = talon.gateway.Gateway.ModifyAgentRequest.class,
      responseType = talon.gateway.Gateway.AgentResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyAgentRequest,
      talon.gateway.Gateway.AgentResponse> getModifyAgentMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyAgentRequest, talon.gateway.Gateway.AgentResponse> getModifyAgentMethod;
    if ((getModifyAgentMethod = GatewayServiceGrpc.getModifyAgentMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getModifyAgentMethod = GatewayServiceGrpc.getModifyAgentMethod) == null) {
          GatewayServiceGrpc.getModifyAgentMethod = getModifyAgentMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ModifyAgentRequest, talon.gateway.Gateway.AgentResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ModifyAgent"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ModifyAgentRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AgentResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ModifyAgent"))
              .build();
        }
      }
    }
    return getModifyAgentMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentsRequest,
      talon.gateway.Gateway.ListAgentsResponse> getListAgentsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListAgents",
      requestType = talon.gateway.Gateway.ListAgentsRequest.class,
      responseType = talon.gateway.Gateway.ListAgentsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentsRequest,
      talon.gateway.Gateway.ListAgentsResponse> getListAgentsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentsRequest, talon.gateway.Gateway.ListAgentsResponse> getListAgentsMethod;
    if ((getListAgentsMethod = GatewayServiceGrpc.getListAgentsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListAgentsMethod = GatewayServiceGrpc.getListAgentsMethod) == null) {
          GatewayServiceGrpc.getListAgentsMethod = getListAgentsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListAgentsRequest, talon.gateway.Gateway.ListAgentsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListAgents"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListAgentsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListAgentsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListAgents"))
              .build();
        }
      }
    }
    return getListAgentsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetKnowledgeRequest,
      talon.gateway.Gateway.KnowledgeResponse> getGetKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetKnowledge",
      requestType = talon.gateway.Gateway.GetKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.KnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetKnowledgeRequest,
      talon.gateway.Gateway.KnowledgeResponse> getGetKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetKnowledgeRequest, talon.gateway.Gateway.KnowledgeResponse> getGetKnowledgeMethod;
    if ((getGetKnowledgeMethod = GatewayServiceGrpc.getGetKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetKnowledgeMethod = GatewayServiceGrpc.getGetKnowledgeMethod) == null) {
          GatewayServiceGrpc.getGetKnowledgeMethod = getGetKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetKnowledgeRequest, talon.gateway.Gateway.KnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.KnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetKnowledge"))
              .build();
        }
      }
    }
    return getGetKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.SearchKnowledgeRequest,
      talon.gateway.Gateway.SearchKnowledgeResponse> getSearchKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "SearchKnowledge",
      requestType = talon.gateway.Gateway.SearchKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.SearchKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.SearchKnowledgeRequest,
      talon.gateway.Gateway.SearchKnowledgeResponse> getSearchKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.SearchKnowledgeRequest, talon.gateway.Gateway.SearchKnowledgeResponse> getSearchKnowledgeMethod;
    if ((getSearchKnowledgeMethod = GatewayServiceGrpc.getSearchKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getSearchKnowledgeMethod = GatewayServiceGrpc.getSearchKnowledgeMethod) == null) {
          GatewayServiceGrpc.getSearchKnowledgeMethod = getSearchKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.SearchKnowledgeRequest, talon.gateway.Gateway.SearchKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "SearchKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SearchKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SearchKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("SearchKnowledge"))
              .build();
        }
      }
    }
    return getSearchKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceKnowledgeRequest,
      talon.gateway.Gateway.NamespaceKnowledgeResponse> getCreateNamespaceKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateNamespaceKnowledge",
      requestType = talon.gateway.Gateway.CreateNamespaceKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.NamespaceKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceKnowledgeRequest,
      talon.gateway.Gateway.NamespaceKnowledgeResponse> getCreateNamespaceKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceKnowledgeRequest, talon.gateway.Gateway.NamespaceKnowledgeResponse> getCreateNamespaceKnowledgeMethod;
    if ((getCreateNamespaceKnowledgeMethod = GatewayServiceGrpc.getCreateNamespaceKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateNamespaceKnowledgeMethod = GatewayServiceGrpc.getCreateNamespaceKnowledgeMethod) == null) {
          GatewayServiceGrpc.getCreateNamespaceKnowledgeMethod = getCreateNamespaceKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateNamespaceKnowledgeRequest, talon.gateway.Gateway.NamespaceKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateNamespaceKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateNamespaceKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.NamespaceKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateNamespaceKnowledge"))
              .build();
        }
      }
    }
    return getCreateNamespaceKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceKnowledgeRequest,
      talon.gateway.Gateway.NamespaceKnowledgeResponse> getGetNamespaceKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetNamespaceKnowledge",
      requestType = talon.gateway.Gateway.GetNamespaceKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.NamespaceKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceKnowledgeRequest,
      talon.gateway.Gateway.NamespaceKnowledgeResponse> getGetNamespaceKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceKnowledgeRequest, talon.gateway.Gateway.NamespaceKnowledgeResponse> getGetNamespaceKnowledgeMethod;
    if ((getGetNamespaceKnowledgeMethod = GatewayServiceGrpc.getGetNamespaceKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetNamespaceKnowledgeMethod = GatewayServiceGrpc.getGetNamespaceKnowledgeMethod) == null) {
          GatewayServiceGrpc.getGetNamespaceKnowledgeMethod = getGetNamespaceKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetNamespaceKnowledgeRequest, talon.gateway.Gateway.NamespaceKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetNamespaceKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetNamespaceKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.NamespaceKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetNamespaceKnowledge"))
              .build();
        }
      }
    }
    return getGetNamespaceKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespaceKnowledgeRequest,
      talon.gateway.Gateway.ListNamespaceKnowledgeResponse> getListNamespaceKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListNamespaceKnowledge",
      requestType = talon.gateway.Gateway.ListNamespaceKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.ListNamespaceKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespaceKnowledgeRequest,
      talon.gateway.Gateway.ListNamespaceKnowledgeResponse> getListNamespaceKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespaceKnowledgeRequest, talon.gateway.Gateway.ListNamespaceKnowledgeResponse> getListNamespaceKnowledgeMethod;
    if ((getListNamespaceKnowledgeMethod = GatewayServiceGrpc.getListNamespaceKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListNamespaceKnowledgeMethod = GatewayServiceGrpc.getListNamespaceKnowledgeMethod) == null) {
          GatewayServiceGrpc.getListNamespaceKnowledgeMethod = getListNamespaceKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListNamespaceKnowledgeRequest, talon.gateway.Gateway.ListNamespaceKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListNamespaceKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListNamespaceKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListNamespaceKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListNamespaceKnowledge"))
              .build();
        }
      }
    }
    return getListNamespaceKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest,
      talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> getDeleteNamespaceKnowledgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteNamespaceKnowledge",
      requestType = talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest.class,
      responseType = talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest,
      talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> getDeleteNamespaceKnowledgeMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest, talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> getDeleteNamespaceKnowledgeMethod;
    if ((getDeleteNamespaceKnowledgeMethod = GatewayServiceGrpc.getDeleteNamespaceKnowledgeMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteNamespaceKnowledgeMethod = GatewayServiceGrpc.getDeleteNamespaceKnowledgeMethod) == null) {
          GatewayServiceGrpc.getDeleteNamespaceKnowledgeMethod = getDeleteNamespaceKnowledgeMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest, talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteNamespaceKnowledge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteNamespaceKnowledge"))
              .build();
        }
      }
    }
    return getDeleteNamespaceKnowledgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateSessionRequest,
      talon.gateway.Gateway.SessionResponse> getCreateSessionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateSession",
      requestType = talon.gateway.Gateway.CreateSessionRequest.class,
      responseType = talon.gateway.Gateway.SessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateSessionRequest,
      talon.gateway.Gateway.SessionResponse> getCreateSessionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateSessionRequest, talon.gateway.Gateway.SessionResponse> getCreateSessionMethod;
    if ((getCreateSessionMethod = GatewayServiceGrpc.getCreateSessionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateSessionMethod = GatewayServiceGrpc.getCreateSessionMethod) == null) {
          GatewayServiceGrpc.getCreateSessionMethod = getCreateSessionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateSessionRequest, talon.gateway.Gateway.SessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateSession"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateSession"))
              .build();
        }
      }
    }
    return getCreateSessionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetSessionRequest,
      talon.gateway.Gateway.SessionResponse> getGetSessionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetSession",
      requestType = talon.gateway.Gateway.GetSessionRequest.class,
      responseType = talon.gateway.Gateway.SessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetSessionRequest,
      talon.gateway.Gateway.SessionResponse> getGetSessionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetSessionRequest, talon.gateway.Gateway.SessionResponse> getGetSessionMethod;
    if ((getGetSessionMethod = GatewayServiceGrpc.getGetSessionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetSessionMethod = GatewayServiceGrpc.getGetSessionMethod) == null) {
          GatewayServiceGrpc.getGetSessionMethod = getGetSessionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetSessionRequest, talon.gateway.Gateway.SessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetSession"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetSession"))
              .build();
        }
      }
    }
    return getGetSessionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionMessagesRequest,
      talon.gateway.Gateway.ListSessionMessagesResponse> getListSessionMessagesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListSessionMessages",
      requestType = talon.gateway.Gateway.ListSessionMessagesRequest.class,
      responseType = talon.gateway.Gateway.ListSessionMessagesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionMessagesRequest,
      talon.gateway.Gateway.ListSessionMessagesResponse> getListSessionMessagesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionMessagesRequest, talon.gateway.Gateway.ListSessionMessagesResponse> getListSessionMessagesMethod;
    if ((getListSessionMessagesMethod = GatewayServiceGrpc.getListSessionMessagesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListSessionMessagesMethod = GatewayServiceGrpc.getListSessionMessagesMethod) == null) {
          GatewayServiceGrpc.getListSessionMessagesMethod = getListSessionMessagesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListSessionMessagesRequest, talon.gateway.Gateway.ListSessionMessagesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListSessionMessages"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSessionMessagesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSessionMessagesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListSessionMessages"))
              .build();
        }
      }
    }
    return getListSessionMessagesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionsRequest,
      talon.gateway.Gateway.ListSessionsResponse> getListSessionsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListSessions",
      requestType = talon.gateway.Gateway.ListSessionsRequest.class,
      responseType = talon.gateway.Gateway.ListSessionsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionsRequest,
      talon.gateway.Gateway.ListSessionsResponse> getListSessionsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSessionsRequest, talon.gateway.Gateway.ListSessionsResponse> getListSessionsMethod;
    if ((getListSessionsMethod = GatewayServiceGrpc.getListSessionsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListSessionsMethod = GatewayServiceGrpc.getListSessionsMethod) == null) {
          GatewayServiceGrpc.getListSessionsMethod = getListSessionsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListSessionsRequest, talon.gateway.Gateway.ListSessionsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListSessions"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSessionsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSessionsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListSessions"))
              .build();
        }
      }
    }
    return getListSessionsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteSessionRequest,
      talon.gateway.Gateway.DeleteSessionResponse> getDeleteSessionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteSession",
      requestType = talon.gateway.Gateway.DeleteSessionRequest.class,
      responseType = talon.gateway.Gateway.DeleteSessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteSessionRequest,
      talon.gateway.Gateway.DeleteSessionResponse> getDeleteSessionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteSessionRequest, talon.gateway.Gateway.DeleteSessionResponse> getDeleteSessionMethod;
    if ((getDeleteSessionMethod = GatewayServiceGrpc.getDeleteSessionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteSessionMethod = GatewayServiceGrpc.getDeleteSessionMethod) == null) {
          GatewayServiceGrpc.getDeleteSessionMethod = getDeleteSessionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteSessionRequest, talon.gateway.Gateway.DeleteSessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteSession"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteSessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteSession"))
              .build();
        }
      }
    }
    return getDeleteSessionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.SendMessageRequest,
      talon.gateway.Gateway.SendMessageResponse> getSendMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "SendMessage",
      requestType = talon.gateway.Gateway.SendMessageRequest.class,
      responseType = talon.gateway.Gateway.SendMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.SendMessageRequest,
      talon.gateway.Gateway.SendMessageResponse> getSendMessageMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.SendMessageRequest, talon.gateway.Gateway.SendMessageResponse> getSendMessageMethod;
    if ((getSendMessageMethod = GatewayServiceGrpc.getSendMessageMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getSendMessageMethod = GatewayServiceGrpc.getSendMessageMethod) == null) {
          GatewayServiceGrpc.getSendMessageMethod = getSendMessageMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.SendMessageRequest, talon.gateway.Gateway.SendMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "SendMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SendMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.SendMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("SendMessage"))
              .build();
        }
      }
    }
    return getSendMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.AppendSessionMessageRequest,
      talon.gateway.Gateway.AppendSessionMessageResponse> getAppendSessionMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "AppendSessionMessage",
      requestType = talon.gateway.Gateway.AppendSessionMessageRequest.class,
      responseType = talon.gateway.Gateway.AppendSessionMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.AppendSessionMessageRequest,
      talon.gateway.Gateway.AppendSessionMessageResponse> getAppendSessionMessageMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.AppendSessionMessageRequest, talon.gateway.Gateway.AppendSessionMessageResponse> getAppendSessionMessageMethod;
    if ((getAppendSessionMessageMethod = GatewayServiceGrpc.getAppendSessionMessageMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getAppendSessionMessageMethod = GatewayServiceGrpc.getAppendSessionMessageMethod) == null) {
          GatewayServiceGrpc.getAppendSessionMessageMethod = getAppendSessionMessageMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.AppendSessionMessageRequest, talon.gateway.Gateway.AppendSessionMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "AppendSessionMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AppendSessionMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AppendSessionMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("AppendSessionMessage"))
              .build();
        }
      }
    }
    return getAppendSessionMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.StopSessionGenerationRequest,
      talon.gateway.Gateway.StopSessionGenerationResponse> getStopSessionGenerationMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StopSessionGeneration",
      requestType = talon.gateway.Gateway.StopSessionGenerationRequest.class,
      responseType = talon.gateway.Gateway.StopSessionGenerationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.StopSessionGenerationRequest,
      talon.gateway.Gateway.StopSessionGenerationResponse> getStopSessionGenerationMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.StopSessionGenerationRequest, talon.gateway.Gateway.StopSessionGenerationResponse> getStopSessionGenerationMethod;
    if ((getStopSessionGenerationMethod = GatewayServiceGrpc.getStopSessionGenerationMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getStopSessionGenerationMethod = GatewayServiceGrpc.getStopSessionGenerationMethod) == null) {
          GatewayServiceGrpc.getStopSessionGenerationMethod = getStopSessionGenerationMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.StopSessionGenerationRequest, talon.gateway.Gateway.StopSessionGenerationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StopSessionGeneration"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StopSessionGenerationRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StopSessionGenerationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("StopSessionGeneration"))
              .build();
        }
      }
    }
    return getStopSessionGenerationMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamSessionParts",
      requestType = talon.gateway.Gateway.StreamSessionPartsRequest.class,
      responseType = talon.events.Events.SessionMessagePartEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsRequest, talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsMethod;
    if ((getStreamSessionPartsMethod = GatewayServiceGrpc.getStreamSessionPartsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getStreamSessionPartsMethod = GatewayServiceGrpc.getStreamSessionPartsMethod) == null) {
          GatewayServiceGrpc.getStreamSessionPartsMethod = getStreamSessionPartsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.StreamSessionPartsRequest, talon.events.Events.SessionMessagePartEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamSessionParts"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StreamSessionPartsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.SessionMessagePartEvent.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("StreamSessionParts"))
              .build();
        }
      }
    }
    return getStreamSessionPartsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsBatchRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsBatchMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamSessionPartsBatch",
      requestType = talon.gateway.Gateway.StreamSessionPartsBatchRequest.class,
      responseType = talon.events.Events.SessionMessagePartEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsBatchRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsBatchMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamSessionPartsBatchRequest, talon.events.Events.SessionMessagePartEvent> getStreamSessionPartsBatchMethod;
    if ((getStreamSessionPartsBatchMethod = GatewayServiceGrpc.getStreamSessionPartsBatchMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getStreamSessionPartsBatchMethod = GatewayServiceGrpc.getStreamSessionPartsBatchMethod) == null) {
          GatewayServiceGrpc.getStreamSessionPartsBatchMethod = getStreamSessionPartsBatchMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.StreamSessionPartsBatchRequest, talon.events.Events.SessionMessagePartEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamSessionPartsBatch"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StreamSessionPartsBatchRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.SessionMessagePartEvent.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("StreamSessionPartsBatch"))
              .build();
        }
      }
    }
    return getStreamSessionPartsBatchMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getCreateChannelMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateChannel",
      requestType = talon.gateway.Gateway.CreateChannelRequest.class,
      responseType = talon.gateway.Gateway.ChannelResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getCreateChannelMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelRequest, talon.gateway.Gateway.ChannelResponse> getCreateChannelMethod;
    if ((getCreateChannelMethod = GatewayServiceGrpc.getCreateChannelMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateChannelMethod = GatewayServiceGrpc.getCreateChannelMethod) == null) {
          GatewayServiceGrpc.getCreateChannelMethod = getCreateChannelMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateChannelRequest, talon.gateway.Gateway.ChannelResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateChannel"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateChannelRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateChannel"))
              .build();
        }
      }
    }
    return getCreateChannelMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getGetChannelMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetChannel",
      requestType = talon.gateway.Gateway.GetChannelRequest.class,
      responseType = talon.gateway.Gateway.ChannelResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getGetChannelMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelRequest, talon.gateway.Gateway.ChannelResponse> getGetChannelMethod;
    if ((getGetChannelMethod = GatewayServiceGrpc.getGetChannelMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetChannelMethod = GatewayServiceGrpc.getGetChannelMethod) == null) {
          GatewayServiceGrpc.getGetChannelMethod = getGetChannelMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetChannelRequest, talon.gateway.Gateway.ChannelResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetChannel"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetChannelRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetChannel"))
              .build();
        }
      }
    }
    return getGetChannelMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getModifyChannelMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ModifyChannel",
      requestType = talon.gateway.Gateway.ModifyChannelRequest.class,
      responseType = talon.gateway.Gateway.ChannelResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelRequest,
      talon.gateway.Gateway.ChannelResponse> getModifyChannelMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelRequest, talon.gateway.Gateway.ChannelResponse> getModifyChannelMethod;
    if ((getModifyChannelMethod = GatewayServiceGrpc.getModifyChannelMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getModifyChannelMethod = GatewayServiceGrpc.getModifyChannelMethod) == null) {
          GatewayServiceGrpc.getModifyChannelMethod = getModifyChannelMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ModifyChannelRequest, talon.gateway.Gateway.ChannelResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ModifyChannel"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ModifyChannelRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ModifyChannel"))
              .build();
        }
      }
    }
    return getModifyChannelMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelsRequest,
      talon.gateway.Gateway.ListChannelsResponse> getListChannelsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListChannels",
      requestType = talon.gateway.Gateway.ListChannelsRequest.class,
      responseType = talon.gateway.Gateway.ListChannelsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelsRequest,
      talon.gateway.Gateway.ListChannelsResponse> getListChannelsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelsRequest, talon.gateway.Gateway.ListChannelsResponse> getListChannelsMethod;
    if ((getListChannelsMethod = GatewayServiceGrpc.getListChannelsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListChannelsMethod = GatewayServiceGrpc.getListChannelsMethod) == null) {
          GatewayServiceGrpc.getListChannelsMethod = getListChannelsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListChannelsRequest, talon.gateway.Gateway.ListChannelsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListChannels"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListChannels"))
              .build();
        }
      }
    }
    return getListChannelsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelRequest,
      talon.gateway.Gateway.DeleteChannelResponse> getDeleteChannelMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteChannel",
      requestType = talon.gateway.Gateway.DeleteChannelRequest.class,
      responseType = talon.gateway.Gateway.DeleteChannelResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelRequest,
      talon.gateway.Gateway.DeleteChannelResponse> getDeleteChannelMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelRequest, talon.gateway.Gateway.DeleteChannelResponse> getDeleteChannelMethod;
    if ((getDeleteChannelMethod = GatewayServiceGrpc.getDeleteChannelMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteChannelMethod = GatewayServiceGrpc.getDeleteChannelMethod) == null) {
          GatewayServiceGrpc.getDeleteChannelMethod = getDeleteChannelMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteChannelRequest, talon.gateway.Gateway.DeleteChannelResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteChannel"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteChannelRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteChannelResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteChannel"))
              .build();
        }
      }
    }
    return getDeleteChannelMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.PostChannelMessageRequest,
      talon.gateway.Gateway.PostChannelMessageResponse> getPostChannelMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "PostChannelMessage",
      requestType = talon.gateway.Gateway.PostChannelMessageRequest.class,
      responseType = talon.gateway.Gateway.PostChannelMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.PostChannelMessageRequest,
      talon.gateway.Gateway.PostChannelMessageResponse> getPostChannelMessageMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.PostChannelMessageRequest, talon.gateway.Gateway.PostChannelMessageResponse> getPostChannelMessageMethod;
    if ((getPostChannelMessageMethod = GatewayServiceGrpc.getPostChannelMessageMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getPostChannelMessageMethod = GatewayServiceGrpc.getPostChannelMessageMethod) == null) {
          GatewayServiceGrpc.getPostChannelMessageMethod = getPostChannelMessageMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.PostChannelMessageRequest, talon.gateway.Gateway.PostChannelMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "PostChannelMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.PostChannelMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.PostChannelMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("PostChannelMessage"))
              .build();
        }
      }
    }
    return getPostChannelMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelMessageRequest,
      talon.gateway.Gateway.ChannelMessageResponse> getGetChannelMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetChannelMessage",
      requestType = talon.gateway.Gateway.GetChannelMessageRequest.class,
      responseType = talon.gateway.Gateway.ChannelMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelMessageRequest,
      talon.gateway.Gateway.ChannelMessageResponse> getGetChannelMessageMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelMessageRequest, talon.gateway.Gateway.ChannelMessageResponse> getGetChannelMessageMethod;
    if ((getGetChannelMessageMethod = GatewayServiceGrpc.getGetChannelMessageMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetChannelMessageMethod = GatewayServiceGrpc.getGetChannelMessageMethod) == null) {
          GatewayServiceGrpc.getGetChannelMessageMethod = getGetChannelMessageMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetChannelMessageRequest, talon.gateway.Gateway.ChannelMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetChannelMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetChannelMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetChannelMessage"))
              .build();
        }
      }
    }
    return getGetChannelMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelMessagesRequest,
      talon.gateway.Gateway.ListChannelMessagesResponse> getListChannelMessagesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListChannelMessages",
      requestType = talon.gateway.Gateway.ListChannelMessagesRequest.class,
      responseType = talon.gateway.Gateway.ListChannelMessagesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelMessagesRequest,
      talon.gateway.Gateway.ListChannelMessagesResponse> getListChannelMessagesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelMessagesRequest, talon.gateway.Gateway.ListChannelMessagesResponse> getListChannelMessagesMethod;
    if ((getListChannelMessagesMethod = GatewayServiceGrpc.getListChannelMessagesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListChannelMessagesMethod = GatewayServiceGrpc.getListChannelMessagesMethod) == null) {
          GatewayServiceGrpc.getListChannelMessagesMethod = getListChannelMessagesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListChannelMessagesRequest, talon.gateway.Gateway.ListChannelMessagesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListChannelMessages"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelMessagesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelMessagesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListChannelMessages"))
              .build();
        }
      }
    }
    return getListChannelMessagesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getCreateChannelSubscriptionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateChannelSubscription",
      requestType = talon.gateway.Gateway.CreateChannelSubscriptionRequest.class,
      responseType = talon.gateway.Gateway.ChannelSubscriptionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getCreateChannelSubscriptionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse> getCreateChannelSubscriptionMethod;
    if ((getCreateChannelSubscriptionMethod = GatewayServiceGrpc.getCreateChannelSubscriptionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateChannelSubscriptionMethod = GatewayServiceGrpc.getCreateChannelSubscriptionMethod) == null) {
          GatewayServiceGrpc.getCreateChannelSubscriptionMethod = getCreateChannelSubscriptionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateChannelSubscription"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateChannelSubscriptionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelSubscriptionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateChannelSubscription"))
              .build();
        }
      }
    }
    return getCreateChannelSubscriptionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getGetChannelSubscriptionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetChannelSubscription",
      requestType = talon.gateway.Gateway.GetChannelSubscriptionRequest.class,
      responseType = talon.gateway.Gateway.ChannelSubscriptionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getGetChannelSubscriptionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse> getGetChannelSubscriptionMethod;
    if ((getGetChannelSubscriptionMethod = GatewayServiceGrpc.getGetChannelSubscriptionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetChannelSubscriptionMethod = GatewayServiceGrpc.getGetChannelSubscriptionMethod) == null) {
          GatewayServiceGrpc.getGetChannelSubscriptionMethod = getGetChannelSubscriptionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetChannelSubscription"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetChannelSubscriptionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelSubscriptionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetChannelSubscription"))
              .build();
        }
      }
    }
    return getGetChannelSubscriptionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getModifyChannelSubscriptionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ModifyChannelSubscription",
      requestType = talon.gateway.Gateway.ModifyChannelSubscriptionRequest.class,
      responseType = talon.gateway.Gateway.ChannelSubscriptionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelSubscriptionRequest,
      talon.gateway.Gateway.ChannelSubscriptionResponse> getModifyChannelSubscriptionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse> getModifyChannelSubscriptionMethod;
    if ((getModifyChannelSubscriptionMethod = GatewayServiceGrpc.getModifyChannelSubscriptionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getModifyChannelSubscriptionMethod = GatewayServiceGrpc.getModifyChannelSubscriptionMethod) == null) {
          GatewayServiceGrpc.getModifyChannelSubscriptionMethod = getModifyChannelSubscriptionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ModifyChannelSubscriptionRequest, talon.gateway.Gateway.ChannelSubscriptionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ModifyChannelSubscription"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ModifyChannelSubscriptionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ChannelSubscriptionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ModifyChannelSubscription"))
              .build();
        }
      }
    }
    return getModifyChannelSubscriptionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelSubscriptionsRequest,
      talon.gateway.Gateway.ListChannelSubscriptionsResponse> getListChannelSubscriptionsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListChannelSubscriptions",
      requestType = talon.gateway.Gateway.ListChannelSubscriptionsRequest.class,
      responseType = talon.gateway.Gateway.ListChannelSubscriptionsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelSubscriptionsRequest,
      talon.gateway.Gateway.ListChannelSubscriptionsResponse> getListChannelSubscriptionsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListChannelSubscriptionsRequest, talon.gateway.Gateway.ListChannelSubscriptionsResponse> getListChannelSubscriptionsMethod;
    if ((getListChannelSubscriptionsMethod = GatewayServiceGrpc.getListChannelSubscriptionsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListChannelSubscriptionsMethod = GatewayServiceGrpc.getListChannelSubscriptionsMethod) == null) {
          GatewayServiceGrpc.getListChannelSubscriptionsMethod = getListChannelSubscriptionsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListChannelSubscriptionsRequest, talon.gateway.Gateway.ListChannelSubscriptionsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListChannelSubscriptions"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelSubscriptionsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListChannelSubscriptionsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListChannelSubscriptions"))
              .build();
        }
      }
    }
    return getListChannelSubscriptionsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelSubscriptionRequest,
      talon.gateway.Gateway.DeleteChannelSubscriptionResponse> getDeleteChannelSubscriptionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteChannelSubscription",
      requestType = talon.gateway.Gateway.DeleteChannelSubscriptionRequest.class,
      responseType = talon.gateway.Gateway.DeleteChannelSubscriptionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelSubscriptionRequest,
      talon.gateway.Gateway.DeleteChannelSubscriptionResponse> getDeleteChannelSubscriptionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteChannelSubscriptionRequest, talon.gateway.Gateway.DeleteChannelSubscriptionResponse> getDeleteChannelSubscriptionMethod;
    if ((getDeleteChannelSubscriptionMethod = GatewayServiceGrpc.getDeleteChannelSubscriptionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteChannelSubscriptionMethod = GatewayServiceGrpc.getDeleteChannelSubscriptionMethod) == null) {
          GatewayServiceGrpc.getDeleteChannelSubscriptionMethod = getDeleteChannelSubscriptionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteChannelSubscriptionRequest, talon.gateway.Gateway.DeleteChannelSubscriptionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteChannelSubscription"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteChannelSubscriptionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteChannelSubscriptionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteChannelSubscription"))
              .build();
        }
      }
    }
    return getDeleteChannelSubscriptionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamChannelEventsRequest,
      talon.events.Events.ChannelEvent> getStreamChannelEventsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamChannelEvents",
      requestType = talon.gateway.Gateway.StreamChannelEventsRequest.class,
      responseType = talon.events.Events.ChannelEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamChannelEventsRequest,
      talon.events.Events.ChannelEvent> getStreamChannelEventsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamChannelEventsRequest, talon.events.Events.ChannelEvent> getStreamChannelEventsMethod;
    if ((getStreamChannelEventsMethod = GatewayServiceGrpc.getStreamChannelEventsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getStreamChannelEventsMethod = GatewayServiceGrpc.getStreamChannelEventsMethod) == null) {
          GatewayServiceGrpc.getStreamChannelEventsMethod = getStreamChannelEventsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.StreamChannelEventsRequest, talon.events.Events.ChannelEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamChannelEvents"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StreamChannelEventsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.ChannelEvent.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("StreamChannelEvents"))
              .build();
        }
      }
    }
    return getStreamChannelEventsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getCreateScheduleMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateSchedule",
      requestType = talon.gateway.Gateway.CreateScheduleRequest.class,
      responseType = talon.gateway.Gateway.ScheduleResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getCreateScheduleMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateScheduleRequest, talon.gateway.Gateway.ScheduleResponse> getCreateScheduleMethod;
    if ((getCreateScheduleMethod = GatewayServiceGrpc.getCreateScheduleMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateScheduleMethod = GatewayServiceGrpc.getCreateScheduleMethod) == null) {
          GatewayServiceGrpc.getCreateScheduleMethod = getCreateScheduleMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateScheduleRequest, talon.gateway.Gateway.ScheduleResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateSchedule"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateScheduleRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ScheduleResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateSchedule"))
              .build();
        }
      }
    }
    return getCreateScheduleMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getGetScheduleMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetSchedule",
      requestType = talon.gateway.Gateway.GetScheduleRequest.class,
      responseType = talon.gateway.Gateway.ScheduleResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getGetScheduleMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetScheduleRequest, talon.gateway.Gateway.ScheduleResponse> getGetScheduleMethod;
    if ((getGetScheduleMethod = GatewayServiceGrpc.getGetScheduleMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetScheduleMethod = GatewayServiceGrpc.getGetScheduleMethod) == null) {
          GatewayServiceGrpc.getGetScheduleMethod = getGetScheduleMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetScheduleRequest, talon.gateway.Gateway.ScheduleResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetSchedule"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetScheduleRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ScheduleResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetSchedule"))
              .build();
        }
      }
    }
    return getGetScheduleMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getModifyScheduleMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ModifySchedule",
      requestType = talon.gateway.Gateway.ModifyScheduleRequest.class,
      responseType = talon.gateway.Gateway.ScheduleResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyScheduleRequest,
      talon.gateway.Gateway.ScheduleResponse> getModifyScheduleMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ModifyScheduleRequest, talon.gateway.Gateway.ScheduleResponse> getModifyScheduleMethod;
    if ((getModifyScheduleMethod = GatewayServiceGrpc.getModifyScheduleMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getModifyScheduleMethod = GatewayServiceGrpc.getModifyScheduleMethod) == null) {
          GatewayServiceGrpc.getModifyScheduleMethod = getModifyScheduleMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ModifyScheduleRequest, talon.gateway.Gateway.ScheduleResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ModifySchedule"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ModifyScheduleRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ScheduleResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ModifySchedule"))
              .build();
        }
      }
    }
    return getModifyScheduleMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSchedulesRequest,
      talon.gateway.Gateway.ListSchedulesResponse> getListSchedulesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListSchedules",
      requestType = talon.gateway.Gateway.ListSchedulesRequest.class,
      responseType = talon.gateway.Gateway.ListSchedulesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSchedulesRequest,
      talon.gateway.Gateway.ListSchedulesResponse> getListSchedulesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListSchedulesRequest, talon.gateway.Gateway.ListSchedulesResponse> getListSchedulesMethod;
    if ((getListSchedulesMethod = GatewayServiceGrpc.getListSchedulesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListSchedulesMethod = GatewayServiceGrpc.getListSchedulesMethod) == null) {
          GatewayServiceGrpc.getListSchedulesMethod = getListSchedulesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListSchedulesRequest, talon.gateway.Gateway.ListSchedulesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListSchedules"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSchedulesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListSchedulesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListSchedules"))
              .build();
        }
      }
    }
    return getListSchedulesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteScheduleRequest,
      talon.gateway.Gateway.DeleteScheduleResponse> getDeleteScheduleMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteSchedule",
      requestType = talon.gateway.Gateway.DeleteScheduleRequest.class,
      responseType = talon.gateway.Gateway.DeleteScheduleResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteScheduleRequest,
      talon.gateway.Gateway.DeleteScheduleResponse> getDeleteScheduleMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteScheduleRequest, talon.gateway.Gateway.DeleteScheduleResponse> getDeleteScheduleMethod;
    if ((getDeleteScheduleMethod = GatewayServiceGrpc.getDeleteScheduleMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteScheduleMethod = GatewayServiceGrpc.getDeleteScheduleMethod) == null) {
          GatewayServiceGrpc.getDeleteScheduleMethod = getDeleteScheduleMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteScheduleRequest, talon.gateway.Gateway.DeleteScheduleResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteSchedule"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteScheduleRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteScheduleResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteSchedule"))
              .build();
        }
      }
    }
    return getDeleteScheduleMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getCreateNamespaceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateNamespace",
      requestType = talon.gateway.Gateway.CreateNamespaceRequest.class,
      responseType = talon.gateway.Gateway.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getCreateNamespaceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateNamespaceRequest, talon.gateway.Gateway.NamespaceResponse> getCreateNamespaceMethod;
    if ((getCreateNamespaceMethod = GatewayServiceGrpc.getCreateNamespaceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateNamespaceMethod = GatewayServiceGrpc.getCreateNamespaceMethod) == null) {
          GatewayServiceGrpc.getCreateNamespaceMethod = getCreateNamespaceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateNamespaceRequest, talon.gateway.Gateway.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateNamespace"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateNamespace"))
              .build();
        }
      }
    }
    return getCreateNamespaceMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getGetNamespaceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetNamespace",
      requestType = talon.gateway.Gateway.GetNamespaceRequest.class,
      responseType = talon.gateway.Gateway.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getGetNamespaceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetNamespaceRequest, talon.gateway.Gateway.NamespaceResponse> getGetNamespaceMethod;
    if ((getGetNamespaceMethod = GatewayServiceGrpc.getGetNamespaceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetNamespaceMethod = GatewayServiceGrpc.getGetNamespaceMethod) == null) {
          GatewayServiceGrpc.getGetNamespaceMethod = getGetNamespaceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetNamespaceRequest, talon.gateway.Gateway.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetNamespace"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetNamespace"))
              .build();
        }
      }
    }
    return getGetNamespaceMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getDeleteNamespaceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteNamespace",
      requestType = talon.gateway.Gateway.DeleteNamespaceRequest.class,
      responseType = talon.gateway.Gateway.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceRequest,
      talon.gateway.Gateway.NamespaceResponse> getDeleteNamespaceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteNamespaceRequest, talon.gateway.Gateway.NamespaceResponse> getDeleteNamespaceMethod;
    if ((getDeleteNamespaceMethod = GatewayServiceGrpc.getDeleteNamespaceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteNamespaceMethod = GatewayServiceGrpc.getDeleteNamespaceMethod) == null) {
          GatewayServiceGrpc.getDeleteNamespaceMethod = getDeleteNamespaceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteNamespaceRequest, talon.gateway.Gateway.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteNamespace"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteNamespace"))
              .build();
        }
      }
    }
    return getDeleteNamespaceMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespacesRequest,
      talon.gateway.Gateway.ListNamespacesResponse> getListNamespacesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListNamespaces",
      requestType = talon.gateway.Gateway.ListNamespacesRequest.class,
      responseType = talon.gateway.Gateway.ListNamespacesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespacesRequest,
      talon.gateway.Gateway.ListNamespacesResponse> getListNamespacesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListNamespacesRequest, talon.gateway.Gateway.ListNamespacesResponse> getListNamespacesMethod;
    if ((getListNamespacesMethod = GatewayServiceGrpc.getListNamespacesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListNamespacesMethod = GatewayServiceGrpc.getListNamespacesMethod) == null) {
          GatewayServiceGrpc.getListNamespacesMethod = getListNamespacesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListNamespacesRequest, talon.gateway.Gateway.ListNamespacesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListNamespaces"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListNamespacesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListNamespacesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListNamespaces"))
              .build();
        }
      }
    }
    return getListNamespacesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentTemplateRequest,
      talon.gateway.Gateway.AgentTemplateResponse> getCreateAgentTemplateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateAgentTemplate",
      requestType = talon.gateway.Gateway.CreateAgentTemplateRequest.class,
      responseType = talon.gateway.Gateway.AgentTemplateResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentTemplateRequest,
      talon.gateway.Gateway.AgentTemplateResponse> getCreateAgentTemplateMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateAgentTemplateRequest, talon.gateway.Gateway.AgentTemplateResponse> getCreateAgentTemplateMethod;
    if ((getCreateAgentTemplateMethod = GatewayServiceGrpc.getCreateAgentTemplateMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateAgentTemplateMethod = GatewayServiceGrpc.getCreateAgentTemplateMethod) == null) {
          GatewayServiceGrpc.getCreateAgentTemplateMethod = getCreateAgentTemplateMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateAgentTemplateRequest, talon.gateway.Gateway.AgentTemplateResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateAgentTemplate"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateAgentTemplateRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AgentTemplateResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateAgentTemplate"))
              .build();
        }
      }
    }
    return getCreateAgentTemplateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentTemplateRequest,
      talon.gateway.Gateway.AgentTemplateResponse> getGetAgentTemplateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetAgentTemplate",
      requestType = talon.gateway.Gateway.GetAgentTemplateRequest.class,
      responseType = talon.gateway.Gateway.AgentTemplateResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentTemplateRequest,
      talon.gateway.Gateway.AgentTemplateResponse> getGetAgentTemplateMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetAgentTemplateRequest, talon.gateway.Gateway.AgentTemplateResponse> getGetAgentTemplateMethod;
    if ((getGetAgentTemplateMethod = GatewayServiceGrpc.getGetAgentTemplateMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetAgentTemplateMethod = GatewayServiceGrpc.getGetAgentTemplateMethod) == null) {
          GatewayServiceGrpc.getGetAgentTemplateMethod = getGetAgentTemplateMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetAgentTemplateRequest, talon.gateway.Gateway.AgentTemplateResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetAgentTemplate"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetAgentTemplateRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.AgentTemplateResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetAgentTemplate"))
              .build();
        }
      }
    }
    return getGetAgentTemplateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentTemplatesRequest,
      talon.gateway.Gateway.ListAgentTemplatesResponse> getListAgentTemplatesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListAgentTemplates",
      requestType = talon.gateway.Gateway.ListAgentTemplatesRequest.class,
      responseType = talon.gateway.Gateway.ListAgentTemplatesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentTemplatesRequest,
      talon.gateway.Gateway.ListAgentTemplatesResponse> getListAgentTemplatesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListAgentTemplatesRequest, talon.gateway.Gateway.ListAgentTemplatesResponse> getListAgentTemplatesMethod;
    if ((getListAgentTemplatesMethod = GatewayServiceGrpc.getListAgentTemplatesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListAgentTemplatesMethod = GatewayServiceGrpc.getListAgentTemplatesMethod) == null) {
          GatewayServiceGrpc.getListAgentTemplatesMethod = getListAgentTemplatesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListAgentTemplatesRequest, talon.gateway.Gateway.ListAgentTemplatesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListAgentTemplates"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListAgentTemplatesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListAgentTemplatesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListAgentTemplates"))
              .build();
        }
      }
    }
    return getListAgentTemplatesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteAgentTemplateRequest,
      talon.gateway.Gateway.DeleteAgentTemplateResponse> getDeleteAgentTemplateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteAgentTemplate",
      requestType = talon.gateway.Gateway.DeleteAgentTemplateRequest.class,
      responseType = talon.gateway.Gateway.DeleteAgentTemplateResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteAgentTemplateRequest,
      talon.gateway.Gateway.DeleteAgentTemplateResponse> getDeleteAgentTemplateMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteAgentTemplateRequest, talon.gateway.Gateway.DeleteAgentTemplateResponse> getDeleteAgentTemplateMethod;
    if ((getDeleteAgentTemplateMethod = GatewayServiceGrpc.getDeleteAgentTemplateMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteAgentTemplateMethod = GatewayServiceGrpc.getDeleteAgentTemplateMethod) == null) {
          GatewayServiceGrpc.getDeleteAgentTemplateMethod = getDeleteAgentTemplateMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteAgentTemplateRequest, talon.gateway.Gateway.DeleteAgentTemplateResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteAgentTemplate"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteAgentTemplateRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteAgentTemplateResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteAgentTemplate"))
              .build();
        }
      }
    }
    return getDeleteAgentTemplateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerRequest,
      talon.gateway.Gateway.McpServerResponse> getCreateMcpServerMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateMcpServer",
      requestType = talon.gateway.Gateway.CreateMcpServerRequest.class,
      responseType = talon.gateway.Gateway.McpServerResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerRequest,
      talon.gateway.Gateway.McpServerResponse> getCreateMcpServerMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerRequest, talon.gateway.Gateway.McpServerResponse> getCreateMcpServerMethod;
    if ((getCreateMcpServerMethod = GatewayServiceGrpc.getCreateMcpServerMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateMcpServerMethod = GatewayServiceGrpc.getCreateMcpServerMethod) == null) {
          GatewayServiceGrpc.getCreateMcpServerMethod = getCreateMcpServerMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateMcpServerRequest, talon.gateway.Gateway.McpServerResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateMcpServer"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateMcpServerRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.McpServerResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateMcpServer"))
              .build();
        }
      }
    }
    return getCreateMcpServerMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerRequest,
      talon.gateway.Gateway.McpServerResponse> getGetMcpServerMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetMcpServer",
      requestType = talon.gateway.Gateway.GetMcpServerRequest.class,
      responseType = talon.gateway.Gateway.McpServerResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerRequest,
      talon.gateway.Gateway.McpServerResponse> getGetMcpServerMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerRequest, talon.gateway.Gateway.McpServerResponse> getGetMcpServerMethod;
    if ((getGetMcpServerMethod = GatewayServiceGrpc.getGetMcpServerMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetMcpServerMethod = GatewayServiceGrpc.getGetMcpServerMethod) == null) {
          GatewayServiceGrpc.getGetMcpServerMethod = getGetMcpServerMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetMcpServerRequest, talon.gateway.Gateway.McpServerResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetMcpServer"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetMcpServerRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.McpServerResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetMcpServer"))
              .build();
        }
      }
    }
    return getGetMcpServerMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServersRequest,
      talon.gateway.Gateway.ListMcpServersResponse> getListMcpServersMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListMcpServers",
      requestType = talon.gateway.Gateway.ListMcpServersRequest.class,
      responseType = talon.gateway.Gateway.ListMcpServersResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServersRequest,
      talon.gateway.Gateway.ListMcpServersResponse> getListMcpServersMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServersRequest, talon.gateway.Gateway.ListMcpServersResponse> getListMcpServersMethod;
    if ((getListMcpServersMethod = GatewayServiceGrpc.getListMcpServersMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListMcpServersMethod = GatewayServiceGrpc.getListMcpServersMethod) == null) {
          GatewayServiceGrpc.getListMcpServersMethod = getListMcpServersMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListMcpServersRequest, talon.gateway.Gateway.ListMcpServersResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListMcpServers"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListMcpServersRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListMcpServersResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListMcpServers"))
              .build();
        }
      }
    }
    return getListMcpServersMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerRequest,
      talon.gateway.Gateway.DeleteMcpServerResponse> getDeleteMcpServerMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteMcpServer",
      requestType = talon.gateway.Gateway.DeleteMcpServerRequest.class,
      responseType = talon.gateway.Gateway.DeleteMcpServerResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerRequest,
      talon.gateway.Gateway.DeleteMcpServerResponse> getDeleteMcpServerMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerRequest, talon.gateway.Gateway.DeleteMcpServerResponse> getDeleteMcpServerMethod;
    if ((getDeleteMcpServerMethod = GatewayServiceGrpc.getDeleteMcpServerMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteMcpServerMethod = GatewayServiceGrpc.getDeleteMcpServerMethod) == null) {
          GatewayServiceGrpc.getDeleteMcpServerMethod = getDeleteMcpServerMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteMcpServerRequest, talon.gateway.Gateway.DeleteMcpServerResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteMcpServer"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteMcpServerRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteMcpServerResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteMcpServer"))
              .build();
        }
      }
    }
    return getDeleteMcpServerMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerBindingRequest,
      talon.gateway.Gateway.McpServerBindingResponse> getCreateMcpServerBindingMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateMcpServerBinding",
      requestType = talon.gateway.Gateway.CreateMcpServerBindingRequest.class,
      responseType = talon.gateway.Gateway.McpServerBindingResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerBindingRequest,
      talon.gateway.Gateway.McpServerBindingResponse> getCreateMcpServerBindingMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateMcpServerBindingRequest, talon.gateway.Gateway.McpServerBindingResponse> getCreateMcpServerBindingMethod;
    if ((getCreateMcpServerBindingMethod = GatewayServiceGrpc.getCreateMcpServerBindingMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateMcpServerBindingMethod = GatewayServiceGrpc.getCreateMcpServerBindingMethod) == null) {
          GatewayServiceGrpc.getCreateMcpServerBindingMethod = getCreateMcpServerBindingMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateMcpServerBindingRequest, talon.gateway.Gateway.McpServerBindingResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateMcpServerBinding"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateMcpServerBindingRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.McpServerBindingResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateMcpServerBinding"))
              .build();
        }
      }
    }
    return getCreateMcpServerBindingMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerBindingRequest,
      talon.gateway.Gateway.McpServerBindingResponse> getGetMcpServerBindingMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetMcpServerBinding",
      requestType = talon.gateway.Gateway.GetMcpServerBindingRequest.class,
      responseType = talon.gateway.Gateway.McpServerBindingResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerBindingRequest,
      talon.gateway.Gateway.McpServerBindingResponse> getGetMcpServerBindingMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetMcpServerBindingRequest, talon.gateway.Gateway.McpServerBindingResponse> getGetMcpServerBindingMethod;
    if ((getGetMcpServerBindingMethod = GatewayServiceGrpc.getGetMcpServerBindingMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetMcpServerBindingMethod = GatewayServiceGrpc.getGetMcpServerBindingMethod) == null) {
          GatewayServiceGrpc.getGetMcpServerBindingMethod = getGetMcpServerBindingMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetMcpServerBindingRequest, talon.gateway.Gateway.McpServerBindingResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetMcpServerBinding"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetMcpServerBindingRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.McpServerBindingResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetMcpServerBinding"))
              .build();
        }
      }
    }
    return getGetMcpServerBindingMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServerBindingsRequest,
      talon.gateway.Gateway.ListMcpServerBindingsResponse> getListMcpServerBindingsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListMcpServerBindings",
      requestType = talon.gateway.Gateway.ListMcpServerBindingsRequest.class,
      responseType = talon.gateway.Gateway.ListMcpServerBindingsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServerBindingsRequest,
      talon.gateway.Gateway.ListMcpServerBindingsResponse> getListMcpServerBindingsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListMcpServerBindingsRequest, talon.gateway.Gateway.ListMcpServerBindingsResponse> getListMcpServerBindingsMethod;
    if ((getListMcpServerBindingsMethod = GatewayServiceGrpc.getListMcpServerBindingsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListMcpServerBindingsMethod = GatewayServiceGrpc.getListMcpServerBindingsMethod) == null) {
          GatewayServiceGrpc.getListMcpServerBindingsMethod = getListMcpServerBindingsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListMcpServerBindingsRequest, talon.gateway.Gateway.ListMcpServerBindingsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListMcpServerBindings"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListMcpServerBindingsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListMcpServerBindingsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListMcpServerBindings"))
              .build();
        }
      }
    }
    return getListMcpServerBindingsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerBindingRequest,
      talon.gateway.Gateway.DeleteMcpServerBindingResponse> getDeleteMcpServerBindingMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteMcpServerBinding",
      requestType = talon.gateway.Gateway.DeleteMcpServerBindingRequest.class,
      responseType = talon.gateway.Gateway.DeleteMcpServerBindingResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerBindingRequest,
      talon.gateway.Gateway.DeleteMcpServerBindingResponse> getDeleteMcpServerBindingMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteMcpServerBindingRequest, talon.gateway.Gateway.DeleteMcpServerBindingResponse> getDeleteMcpServerBindingMethod;
    if ((getDeleteMcpServerBindingMethod = GatewayServiceGrpc.getDeleteMcpServerBindingMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteMcpServerBindingMethod = GatewayServiceGrpc.getDeleteMcpServerBindingMethod) == null) {
          GatewayServiceGrpc.getDeleteMcpServerBindingMethod = getDeleteMcpServerBindingMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteMcpServerBindingRequest, talon.gateway.Gateway.DeleteMcpServerBindingResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteMcpServerBinding"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteMcpServerBindingRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteMcpServerBindingResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteMcpServerBinding"))
              .build();
        }
      }
    }
    return getDeleteMcpServerBindingMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static GatewayServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<GatewayServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<GatewayServiceStub>() {
        @java.lang.Override
        public GatewayServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new GatewayServiceStub(channel, callOptions);
        }
      };
    return GatewayServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static GatewayServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<GatewayServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<GatewayServiceBlockingV2Stub>() {
        @java.lang.Override
        public GatewayServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new GatewayServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return GatewayServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static GatewayServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<GatewayServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<GatewayServiceBlockingStub>() {
        @java.lang.Override
        public GatewayServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new GatewayServiceBlockingStub(channel, callOptions);
        }
      };
    return GatewayServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static GatewayServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<GatewayServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<GatewayServiceFutureStub>() {
        @java.lang.Override
        public GatewayServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new GatewayServiceFutureStub(channel, callOptions);
        }
      };
    return GatewayServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     * <pre>
     * Agent Lifecycle
     * </pre>
     */
    default void createAgent(talon.gateway.Gateway.CreateAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateAgentMethod(), responseObserver);
    }

    /**
     */
    default void getAgent(talon.gateway.Gateway.GetAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.GetAgentResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetAgentMethod(), responseObserver);
    }

    /**
     */
    default void modifyAgent(talon.gateway.Gateway.ModifyAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getModifyAgentMethod(), responseObserver);
    }

    /**
     */
    default void listAgents(talon.gateway.Gateway.ListAgentsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListAgentsMethod(), responseObserver);
    }

    /**
     * <pre>
     * Agent Knowledge
     * </pre>
     */
    default void getKnowledge(talon.gateway.Gateway.GetKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.KnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetKnowledgeMethod(), responseObserver);
    }

    /**
     */
    default void searchKnowledge(talon.gateway.Gateway.SearchKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SearchKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getSearchKnowledgeMethod(), responseObserver);
    }

    /**
     */
    default void createNamespaceKnowledge(talon.gateway.Gateway.CreateNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateNamespaceKnowledgeMethod(), responseObserver);
    }

    /**
     */
    default void getNamespaceKnowledge(talon.gateway.Gateway.GetNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetNamespaceKnowledgeMethod(), responseObserver);
    }

    /**
     */
    default void listNamespaceKnowledge(talon.gateway.Gateway.ListNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListNamespaceKnowledgeMethod(), responseObserver);
    }

    /**
     */
    default void deleteNamespaceKnowledge(talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteNamespaceKnowledgeMethod(), responseObserver);
    }

    /**
     * <pre>
     * Agent Sessions
     * </pre>
     */
    default void createSession(talon.gateway.Gateway.CreateSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateSessionMethod(), responseObserver);
    }

    /**
     */
    default void getSession(talon.gateway.Gateway.GetSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetSessionMethod(), responseObserver);
    }

    /**
     */
    default void listSessionMessages(talon.gateway.Gateway.ListSessionMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionMessagesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListSessionMessagesMethod(), responseObserver);
    }

    /**
     */
    default void listSessions(talon.gateway.Gateway.ListSessionsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListSessionsMethod(), responseObserver);
    }

    /**
     */
    default void deleteSession(talon.gateway.Gateway.DeleteSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteSessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteSessionMethod(), responseObserver);
    }

    /**
     * <pre>
     * Interactive Comm
     * </pre>
     */
    default void sendMessage(talon.gateway.Gateway.SendMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SendMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getSendMessageMethod(), responseObserver);
    }

    /**
     */
    default void appendSessionMessage(talon.gateway.Gateway.AppendSessionMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AppendSessionMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getAppendSessionMessageMethod(), responseObserver);
    }

    /**
     */
    default void stopSessionGeneration(talon.gateway.Gateway.StopSessionGenerationRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.StopSessionGenerationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStopSessionGenerationMethod(), responseObserver);
    }

    /**
     */
    default void streamSessionParts(talon.gateway.Gateway.StreamSessionPartsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamSessionPartsMethod(), responseObserver);
    }

    /**
     */
    default void streamSessionPartsBatch(talon.gateway.Gateway.StreamSessionPartsBatchRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamSessionPartsBatchMethod(), responseObserver);
    }

    /**
     * <pre>
     * Channels
     * </pre>
     */
    default void createChannel(talon.gateway.Gateway.CreateChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateChannelMethod(), responseObserver);
    }

    /**
     */
    default void getChannel(talon.gateway.Gateway.GetChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetChannelMethod(), responseObserver);
    }

    /**
     */
    default void modifyChannel(talon.gateway.Gateway.ModifyChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getModifyChannelMethod(), responseObserver);
    }

    /**
     */
    default void listChannels(talon.gateway.Gateway.ListChannelsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListChannelsMethod(), responseObserver);
    }

    /**
     */
    default void deleteChannel(talon.gateway.Gateway.DeleteChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteChannelMethod(), responseObserver);
    }

    /**
     */
    default void postChannelMessage(talon.gateway.Gateway.PostChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.PostChannelMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getPostChannelMessageMethod(), responseObserver);
    }

    /**
     */
    default void getChannelMessage(talon.gateway.Gateway.GetChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetChannelMessageMethod(), responseObserver);
    }

    /**
     */
    default void listChannelMessages(talon.gateway.Gateway.ListChannelMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelMessagesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListChannelMessagesMethod(), responseObserver);
    }

    /**
     */
    default void createChannelSubscription(talon.gateway.Gateway.CreateChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateChannelSubscriptionMethod(), responseObserver);
    }

    /**
     */
    default void getChannelSubscription(talon.gateway.Gateway.GetChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetChannelSubscriptionMethod(), responseObserver);
    }

    /**
     */
    default void modifyChannelSubscription(talon.gateway.Gateway.ModifyChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getModifyChannelSubscriptionMethod(), responseObserver);
    }

    /**
     */
    default void listChannelSubscriptions(talon.gateway.Gateway.ListChannelSubscriptionsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelSubscriptionsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListChannelSubscriptionsMethod(), responseObserver);
    }

    /**
     */
    default void deleteChannelSubscription(talon.gateway.Gateway.DeleteChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteChannelSubscriptionMethod(), responseObserver);
    }

    /**
     */
    default void streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamChannelEventsMethod(), responseObserver);
    }

    /**
     * <pre>
     * Schedules
     * </pre>
     */
    default void createSchedule(talon.gateway.Gateway.CreateScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateScheduleMethod(), responseObserver);
    }

    /**
     */
    default void getSchedule(talon.gateway.Gateway.GetScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetScheduleMethod(), responseObserver);
    }

    /**
     */
    default void modifySchedule(talon.gateway.Gateway.ModifyScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getModifyScheduleMethod(), responseObserver);
    }

    /**
     */
    default void listSchedules(talon.gateway.Gateway.ListSchedulesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSchedulesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListSchedulesMethod(), responseObserver);
    }

    /**
     */
    default void deleteSchedule(talon.gateway.Gateway.DeleteScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteScheduleResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteScheduleMethod(), responseObserver);
    }

    /**
     * <pre>
     * Namespaces
     * </pre>
     */
    default void createNamespace(talon.gateway.Gateway.CreateNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateNamespaceMethod(), responseObserver);
    }

    /**
     */
    default void getNamespace(talon.gateway.Gateway.GetNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetNamespaceMethod(), responseObserver);
    }

    /**
     */
    default void deleteNamespace(talon.gateway.Gateway.DeleteNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteNamespaceMethod(), responseObserver);
    }

    /**
     */
    default void listNamespaces(talon.gateway.Gateway.ListNamespacesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespacesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListNamespacesMethod(), responseObserver);
    }

    /**
     * <pre>
     * Agent Templates
     * </pre>
     */
    default void createAgentTemplate(talon.gateway.Gateway.CreateAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateAgentTemplateMethod(), responseObserver);
    }

    /**
     */
    default void getAgentTemplate(talon.gateway.Gateway.GetAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetAgentTemplateMethod(), responseObserver);
    }

    /**
     */
    default void listAgentTemplates(talon.gateway.Gateway.ListAgentTemplatesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentTemplatesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListAgentTemplatesMethod(), responseObserver);
    }

    /**
     */
    default void deleteAgentTemplate(talon.gateway.Gateway.DeleteAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteAgentTemplateResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteAgentTemplateMethod(), responseObserver);
    }

    /**
     * <pre>
     * MCP Servers
     * </pre>
     */
    default void createMcpServer(talon.gateway.Gateway.CreateMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateMcpServerMethod(), responseObserver);
    }

    /**
     */
    default void getMcpServer(talon.gateway.Gateway.GetMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMcpServerMethod(), responseObserver);
    }

    /**
     */
    default void listMcpServers(talon.gateway.Gateway.ListMcpServersRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServersResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMcpServersMethod(), responseObserver);
    }

    /**
     */
    default void deleteMcpServer(talon.gateway.Gateway.DeleteMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteMcpServerMethod(), responseObserver);
    }

    /**
     */
    default void createMcpServerBinding(talon.gateway.Gateway.CreateMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateMcpServerBindingMethod(), responseObserver);
    }

    /**
     */
    default void getMcpServerBinding(talon.gateway.Gateway.GetMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMcpServerBindingMethod(), responseObserver);
    }

    /**
     */
    default void listMcpServerBindings(talon.gateway.Gateway.ListMcpServerBindingsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServerBindingsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMcpServerBindingsMethod(), responseObserver);
    }

    /**
     */
    default void deleteMcpServerBinding(talon.gateway.Gateway.DeleteMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerBindingResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteMcpServerBindingMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service GatewayService.
   */
  public static abstract class GatewayServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return GatewayServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service GatewayService.
   */
  public static final class GatewayServiceStub
      extends io.grpc.stub.AbstractAsyncStub<GatewayServiceStub> {
    private GatewayServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected GatewayServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new GatewayServiceStub(channel, callOptions);
    }

    /**
     * <pre>
     * Agent Lifecycle
     * </pre>
     */
    public void createAgent(talon.gateway.Gateway.CreateAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateAgentMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getAgent(talon.gateway.Gateway.GetAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.GetAgentResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetAgentMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void modifyAgent(talon.gateway.Gateway.ModifyAgentRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getModifyAgentMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listAgents(talon.gateway.Gateway.ListAgentsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListAgentsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Agent Knowledge
     * </pre>
     */
    public void getKnowledge(talon.gateway.Gateway.GetKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.KnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void searchKnowledge(talon.gateway.Gateway.SearchKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SearchKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getSearchKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void createNamespaceKnowledge(talon.gateway.Gateway.CreateNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateNamespaceKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getNamespaceKnowledge(talon.gateway.Gateway.GetNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetNamespaceKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listNamespaceKnowledge(talon.gateway.Gateway.ListNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListNamespaceKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteNamespaceKnowledge(talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteNamespaceKnowledgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Agent Sessions
     * </pre>
     */
    public void createSession(talon.gateway.Gateway.CreateSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateSessionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getSession(talon.gateway.Gateway.GetSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetSessionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listSessionMessages(talon.gateway.Gateway.ListSessionMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionMessagesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListSessionMessagesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listSessions(talon.gateway.Gateway.ListSessionsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListSessionsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteSession(talon.gateway.Gateway.DeleteSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteSessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteSessionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Interactive Comm
     * </pre>
     */
    public void sendMessage(talon.gateway.Gateway.SendMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.SendMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getSendMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void appendSessionMessage(talon.gateway.Gateway.AppendSessionMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AppendSessionMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getAppendSessionMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void stopSessionGeneration(talon.gateway.Gateway.StopSessionGenerationRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.StopSessionGenerationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getStopSessionGenerationMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamSessionParts(talon.gateway.Gateway.StreamSessionPartsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamSessionPartsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamSessionPartsBatch(talon.gateway.Gateway.StreamSessionPartsBatchRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamSessionPartsBatchMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Channels
     * </pre>
     */
    public void createChannel(talon.gateway.Gateway.CreateChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateChannelMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getChannel(talon.gateway.Gateway.GetChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetChannelMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void modifyChannel(talon.gateway.Gateway.ModifyChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getModifyChannelMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listChannels(talon.gateway.Gateway.ListChannelsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListChannelsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteChannel(talon.gateway.Gateway.DeleteChannelRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteChannelMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void postChannelMessage(talon.gateway.Gateway.PostChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.PostChannelMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getPostChannelMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getChannelMessage(talon.gateway.Gateway.GetChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetChannelMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listChannelMessages(talon.gateway.Gateway.ListChannelMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelMessagesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListChannelMessagesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void createChannelSubscription(talon.gateway.Gateway.CreateChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateChannelSubscriptionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getChannelSubscription(talon.gateway.Gateway.GetChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetChannelSubscriptionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void modifyChannelSubscription(talon.gateway.Gateway.ModifyChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getModifyChannelSubscriptionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listChannelSubscriptions(talon.gateway.Gateway.ListChannelSubscriptionsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelSubscriptionsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListChannelSubscriptionsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteChannelSubscription(talon.gateway.Gateway.DeleteChannelSubscriptionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelSubscriptionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteChannelSubscriptionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamChannelEventsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Schedules
     * </pre>
     */
    public void createSchedule(talon.gateway.Gateway.CreateScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateScheduleMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getSchedule(talon.gateway.Gateway.GetScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetScheduleMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void modifySchedule(talon.gateway.Gateway.ModifyScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getModifyScheduleMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listSchedules(talon.gateway.Gateway.ListSchedulesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSchedulesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListSchedulesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteSchedule(talon.gateway.Gateway.DeleteScheduleRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteScheduleResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteScheduleMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Namespaces
     * </pre>
     */
    public void createNamespace(talon.gateway.Gateway.CreateNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateNamespaceMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getNamespace(talon.gateway.Gateway.GetNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetNamespaceMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteNamespace(talon.gateway.Gateway.DeleteNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteNamespaceMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listNamespaces(talon.gateway.Gateway.ListNamespacesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespacesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListNamespacesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Agent Templates
     * </pre>
     */
    public void createAgentTemplate(talon.gateway.Gateway.CreateAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateAgentTemplateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getAgentTemplate(talon.gateway.Gateway.GetAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetAgentTemplateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listAgentTemplates(talon.gateway.Gateway.ListAgentTemplatesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentTemplatesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListAgentTemplatesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteAgentTemplate(talon.gateway.Gateway.DeleteAgentTemplateRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteAgentTemplateResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteAgentTemplateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * MCP Servers
     * </pre>
     */
    public void createMcpServer(talon.gateway.Gateway.CreateMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateMcpServerMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getMcpServer(talon.gateway.Gateway.GetMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMcpServerMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listMcpServers(talon.gateway.Gateway.ListMcpServersRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServersResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMcpServersMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteMcpServer(talon.gateway.Gateway.DeleteMcpServerRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteMcpServerMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void createMcpServerBinding(talon.gateway.Gateway.CreateMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateMcpServerBindingMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getMcpServerBinding(talon.gateway.Gateway.GetMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMcpServerBindingMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listMcpServerBindings(talon.gateway.Gateway.ListMcpServerBindingsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServerBindingsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMcpServerBindingsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteMcpServerBinding(talon.gateway.Gateway.DeleteMcpServerBindingRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerBindingResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteMcpServerBindingMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service GatewayService.
   */
  public static final class GatewayServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<GatewayServiceBlockingV2Stub> {
    private GatewayServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected GatewayServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new GatewayServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     * <pre>
     * Agent Lifecycle
     * </pre>
     */
    public talon.gateway.Gateway.AgentResponse createAgent(talon.gateway.Gateway.CreateAgentRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.GetAgentResponse getAgent(talon.gateway.Gateway.GetAgentRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AgentResponse modifyAgent(talon.gateway.Gateway.ModifyAgentRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getModifyAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListAgentsResponse listAgents(talon.gateway.Gateway.ListAgentsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListAgentsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Knowledge
     * </pre>
     */
    public talon.gateway.Gateway.KnowledgeResponse getKnowledge(talon.gateway.Gateway.GetKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.SearchKnowledgeResponse searchKnowledge(talon.gateway.Gateway.SearchKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getSearchKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceKnowledgeResponse createNamespaceKnowledge(talon.gateway.Gateway.CreateNamespaceKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceKnowledgeResponse getNamespaceKnowledge(talon.gateway.Gateway.GetNamespaceKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListNamespaceKnowledgeResponse listNamespaceKnowledge(talon.gateway.Gateway.ListNamespaceKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse deleteNamespaceKnowledge(talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Sessions
     * </pre>
     */
    public talon.gateway.Gateway.SessionResponse createSession(talon.gateway.Gateway.CreateSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateSessionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.SessionResponse getSession(talon.gateway.Gateway.GetSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetSessionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSessionMessagesResponse listSessionMessages(talon.gateway.Gateway.ListSessionMessagesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListSessionMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSessionsResponse listSessions(talon.gateway.Gateway.ListSessionsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListSessionsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteSessionResponse deleteSession(talon.gateway.Gateway.DeleteSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteSessionMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Interactive Comm
     * </pre>
     */
    public talon.gateway.Gateway.SendMessageResponse sendMessage(talon.gateway.Gateway.SendMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getSendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AppendSessionMessageResponse appendSessionMessage(talon.gateway.Gateway.AppendSessionMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getAppendSessionMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.StopSessionGenerationResponse stopSessionGeneration(talon.gateway.Gateway.StopSessionGenerationRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getStopSessionGenerationMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.SessionMessagePartEvent>
        streamSessionParts(talon.gateway.Gateway.StreamSessionPartsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamSessionPartsMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.SessionMessagePartEvent>
        streamSessionPartsBatch(talon.gateway.Gateway.StreamSessionPartsBatchRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamSessionPartsBatchMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Channels
     * </pre>
     */
    public talon.gateway.Gateway.ChannelResponse createChannel(talon.gateway.Gateway.CreateChannelRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelResponse getChannel(talon.gateway.Gateway.GetChannelRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelResponse modifyChannel(talon.gateway.Gateway.ModifyChannelRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getModifyChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelsResponse listChannels(talon.gateway.Gateway.ListChannelsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListChannelsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteChannelResponse deleteChannel(talon.gateway.Gateway.DeleteChannelRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.PostChannelMessageResponse postChannelMessage(talon.gateway.Gateway.PostChannelMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getPostChannelMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelMessageResponse getChannelMessage(talon.gateway.Gateway.GetChannelMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetChannelMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelMessagesResponse listChannelMessages(talon.gateway.Gateway.ListChannelMessagesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListChannelMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse createChannelSubscription(talon.gateway.Gateway.CreateChannelSubscriptionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse getChannelSubscription(talon.gateway.Gateway.GetChannelSubscriptionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse modifyChannelSubscription(talon.gateway.Gateway.ModifyChannelSubscriptionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getModifyChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelSubscriptionsResponse listChannelSubscriptions(talon.gateway.Gateway.ListChannelSubscriptionsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListChannelSubscriptionsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteChannelSubscriptionResponse deleteChannelSubscription(talon.gateway.Gateway.DeleteChannelSubscriptionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.ChannelEvent>
        streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamChannelEventsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Schedules
     * </pre>
     */
    public talon.gateway.Gateway.ScheduleResponse createSchedule(talon.gateway.Gateway.CreateScheduleRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ScheduleResponse getSchedule(talon.gateway.Gateway.GetScheduleRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ScheduleResponse modifySchedule(talon.gateway.Gateway.ModifyScheduleRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getModifyScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSchedulesResponse listSchedules(talon.gateway.Gateway.ListSchedulesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListSchedulesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteScheduleResponse deleteSchedule(talon.gateway.Gateway.DeleteScheduleRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteScheduleMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Namespaces
     * </pre>
     */
    public talon.gateway.Gateway.NamespaceResponse createNamespace(talon.gateway.Gateway.CreateNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceResponse getNamespace(talon.gateway.Gateway.GetNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceResponse deleteNamespace(talon.gateway.Gateway.DeleteNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListNamespacesResponse listNamespaces(talon.gateway.Gateway.ListNamespacesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListNamespacesMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Templates
     * </pre>
     */
    public talon.gateway.Gateway.AgentTemplateResponse createAgentTemplate(talon.gateway.Gateway.CreateAgentTemplateRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AgentTemplateResponse getAgentTemplate(talon.gateway.Gateway.GetAgentTemplateRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListAgentTemplatesResponse listAgentTemplates(talon.gateway.Gateway.ListAgentTemplatesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListAgentTemplatesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteAgentTemplateResponse deleteAgentTemplate(talon.gateway.Gateway.DeleteAgentTemplateRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * MCP Servers
     * </pre>
     */
    public talon.gateway.Gateway.McpServerResponse createMcpServer(talon.gateway.Gateway.CreateMcpServerRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerResponse getMcpServer(talon.gateway.Gateway.GetMcpServerRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListMcpServersResponse listMcpServers(talon.gateway.Gateway.ListMcpServersRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMcpServersMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteMcpServerResponse deleteMcpServer(talon.gateway.Gateway.DeleteMcpServerRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerBindingResponse createMcpServerBinding(talon.gateway.Gateway.CreateMcpServerBindingRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateMcpServerBindingMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerBindingResponse getMcpServerBinding(talon.gateway.Gateway.GetMcpServerBindingRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMcpServerBindingMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListMcpServerBindingsResponse listMcpServerBindings(talon.gateway.Gateway.ListMcpServerBindingsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMcpServerBindingsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteMcpServerBindingResponse deleteMcpServerBinding(talon.gateway.Gateway.DeleteMcpServerBindingRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteMcpServerBindingMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service GatewayService.
   */
  public static final class GatewayServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<GatewayServiceBlockingStub> {
    private GatewayServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected GatewayServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new GatewayServiceBlockingStub(channel, callOptions);
    }

    /**
     * <pre>
     * Agent Lifecycle
     * </pre>
     */
    public talon.gateway.Gateway.AgentResponse createAgent(talon.gateway.Gateway.CreateAgentRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.GetAgentResponse getAgent(talon.gateway.Gateway.GetAgentRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AgentResponse modifyAgent(talon.gateway.Gateway.ModifyAgentRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getModifyAgentMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListAgentsResponse listAgents(talon.gateway.Gateway.ListAgentsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListAgentsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Knowledge
     * </pre>
     */
    public talon.gateway.Gateway.KnowledgeResponse getKnowledge(talon.gateway.Gateway.GetKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.SearchKnowledgeResponse searchKnowledge(talon.gateway.Gateway.SearchKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getSearchKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceKnowledgeResponse createNamespaceKnowledge(talon.gateway.Gateway.CreateNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceKnowledgeResponse getNamespaceKnowledge(talon.gateway.Gateway.GetNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListNamespaceKnowledgeResponse listNamespaceKnowledge(talon.gateway.Gateway.ListNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse deleteNamespaceKnowledge(talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteNamespaceKnowledgeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Sessions
     * </pre>
     */
    public talon.gateway.Gateway.SessionResponse createSession(talon.gateway.Gateway.CreateSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateSessionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.SessionResponse getSession(talon.gateway.Gateway.GetSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetSessionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSessionMessagesResponse listSessionMessages(talon.gateway.Gateway.ListSessionMessagesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListSessionMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSessionsResponse listSessions(talon.gateway.Gateway.ListSessionsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListSessionsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteSessionResponse deleteSession(talon.gateway.Gateway.DeleteSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteSessionMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Interactive Comm
     * </pre>
     */
    public talon.gateway.Gateway.SendMessageResponse sendMessage(talon.gateway.Gateway.SendMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getSendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AppendSessionMessageResponse appendSessionMessage(talon.gateway.Gateway.AppendSessionMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getAppendSessionMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.StopSessionGenerationResponse stopSessionGeneration(talon.gateway.Gateway.StopSessionGenerationRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getStopSessionGenerationMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.SessionMessagePartEvent> streamSessionParts(
        talon.gateway.Gateway.StreamSessionPartsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamSessionPartsMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.SessionMessagePartEvent> streamSessionPartsBatch(
        talon.gateway.Gateway.StreamSessionPartsBatchRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamSessionPartsBatchMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Channels
     * </pre>
     */
    public talon.gateway.Gateway.ChannelResponse createChannel(talon.gateway.Gateway.CreateChannelRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelResponse getChannel(talon.gateway.Gateway.GetChannelRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelResponse modifyChannel(talon.gateway.Gateway.ModifyChannelRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getModifyChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelsResponse listChannels(talon.gateway.Gateway.ListChannelsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListChannelsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteChannelResponse deleteChannel(talon.gateway.Gateway.DeleteChannelRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteChannelMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.PostChannelMessageResponse postChannelMessage(talon.gateway.Gateway.PostChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getPostChannelMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelMessageResponse getChannelMessage(talon.gateway.Gateway.GetChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetChannelMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelMessagesResponse listChannelMessages(talon.gateway.Gateway.ListChannelMessagesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListChannelMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse createChannelSubscription(talon.gateway.Gateway.CreateChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse getChannelSubscription(talon.gateway.Gateway.GetChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ChannelSubscriptionResponse modifyChannelSubscription(talon.gateway.Gateway.ModifyChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getModifyChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListChannelSubscriptionsResponse listChannelSubscriptions(talon.gateway.Gateway.ListChannelSubscriptionsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListChannelSubscriptionsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteChannelSubscriptionResponse deleteChannelSubscription(talon.gateway.Gateway.DeleteChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteChannelSubscriptionMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.ChannelEvent> streamChannelEvents(
        talon.gateway.Gateway.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamChannelEventsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Schedules
     * </pre>
     */
    public talon.gateway.Gateway.ScheduleResponse createSchedule(talon.gateway.Gateway.CreateScheduleRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ScheduleResponse getSchedule(talon.gateway.Gateway.GetScheduleRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ScheduleResponse modifySchedule(talon.gateway.Gateway.ModifyScheduleRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getModifyScheduleMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListSchedulesResponse listSchedules(talon.gateway.Gateway.ListSchedulesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListSchedulesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteScheduleResponse deleteSchedule(talon.gateway.Gateway.DeleteScheduleRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteScheduleMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Namespaces
     * </pre>
     */
    public talon.gateway.Gateway.NamespaceResponse createNamespace(talon.gateway.Gateway.CreateNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceResponse getNamespace(talon.gateway.Gateway.GetNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.NamespaceResponse deleteNamespace(talon.gateway.Gateway.DeleteNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteNamespaceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListNamespacesResponse listNamespaces(talon.gateway.Gateway.ListNamespacesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListNamespacesMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Agent Templates
     * </pre>
     */
    public talon.gateway.Gateway.AgentTemplateResponse createAgentTemplate(talon.gateway.Gateway.CreateAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.AgentTemplateResponse getAgentTemplate(talon.gateway.Gateway.GetAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListAgentTemplatesResponse listAgentTemplates(talon.gateway.Gateway.ListAgentTemplatesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListAgentTemplatesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteAgentTemplateResponse deleteAgentTemplate(talon.gateway.Gateway.DeleteAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteAgentTemplateMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * MCP Servers
     * </pre>
     */
    public talon.gateway.Gateway.McpServerResponse createMcpServer(talon.gateway.Gateway.CreateMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerResponse getMcpServer(talon.gateway.Gateway.GetMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListMcpServersResponse listMcpServers(talon.gateway.Gateway.ListMcpServersRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMcpServersMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteMcpServerResponse deleteMcpServer(talon.gateway.Gateway.DeleteMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteMcpServerMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerBindingResponse createMcpServerBinding(talon.gateway.Gateway.CreateMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateMcpServerBindingMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.McpServerBindingResponse getMcpServerBinding(talon.gateway.Gateway.GetMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMcpServerBindingMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListMcpServerBindingsResponse listMcpServerBindings(talon.gateway.Gateway.ListMcpServerBindingsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMcpServerBindingsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteMcpServerBindingResponse deleteMcpServerBinding(talon.gateway.Gateway.DeleteMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteMcpServerBindingMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service GatewayService.
   */
  public static final class GatewayServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<GatewayServiceFutureStub> {
    private GatewayServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected GatewayServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new GatewayServiceFutureStub(channel, callOptions);
    }

    /**
     * <pre>
     * Agent Lifecycle
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.AgentResponse> createAgent(
        talon.gateway.Gateway.CreateAgentRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateAgentMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.GetAgentResponse> getAgent(
        talon.gateway.Gateway.GetAgentRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetAgentMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.AgentResponse> modifyAgent(
        talon.gateway.Gateway.ModifyAgentRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getModifyAgentMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListAgentsResponse> listAgents(
        talon.gateway.Gateway.ListAgentsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListAgentsMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Agent Knowledge
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.KnowledgeResponse> getKnowledge(
        talon.gateway.Gateway.GetKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.SearchKnowledgeResponse> searchKnowledge(
        talon.gateway.Gateway.SearchKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getSearchKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.NamespaceKnowledgeResponse> createNamespaceKnowledge(
        talon.gateway.Gateway.CreateNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateNamespaceKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.NamespaceKnowledgeResponse> getNamespaceKnowledge(
        talon.gateway.Gateway.GetNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetNamespaceKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListNamespaceKnowledgeResponse> listNamespaceKnowledge(
        talon.gateway.Gateway.ListNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListNamespaceKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse> deleteNamespaceKnowledge(
        talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteNamespaceKnowledgeMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Agent Sessions
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.SessionResponse> createSession(
        talon.gateway.Gateway.CreateSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateSessionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.SessionResponse> getSession(
        talon.gateway.Gateway.GetSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetSessionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListSessionMessagesResponse> listSessionMessages(
        talon.gateway.Gateway.ListSessionMessagesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListSessionMessagesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListSessionsResponse> listSessions(
        talon.gateway.Gateway.ListSessionsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListSessionsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteSessionResponse> deleteSession(
        talon.gateway.Gateway.DeleteSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteSessionMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Interactive Comm
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.SendMessageResponse> sendMessage(
        talon.gateway.Gateway.SendMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getSendMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.AppendSessionMessageResponse> appendSessionMessage(
        talon.gateway.Gateway.AppendSessionMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getAppendSessionMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.StopSessionGenerationResponse> stopSessionGeneration(
        talon.gateway.Gateway.StopSessionGenerationRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getStopSessionGenerationMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Channels
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelResponse> createChannel(
        talon.gateway.Gateway.CreateChannelRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateChannelMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelResponse> getChannel(
        talon.gateway.Gateway.GetChannelRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetChannelMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelResponse> modifyChannel(
        talon.gateway.Gateway.ModifyChannelRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getModifyChannelMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListChannelsResponse> listChannels(
        talon.gateway.Gateway.ListChannelsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListChannelsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteChannelResponse> deleteChannel(
        talon.gateway.Gateway.DeleteChannelRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteChannelMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.PostChannelMessageResponse> postChannelMessage(
        talon.gateway.Gateway.PostChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getPostChannelMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelMessageResponse> getChannelMessage(
        talon.gateway.Gateway.GetChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetChannelMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListChannelMessagesResponse> listChannelMessages(
        talon.gateway.Gateway.ListChannelMessagesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListChannelMessagesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelSubscriptionResponse> createChannelSubscription(
        talon.gateway.Gateway.CreateChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateChannelSubscriptionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelSubscriptionResponse> getChannelSubscription(
        talon.gateway.Gateway.GetChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetChannelSubscriptionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ChannelSubscriptionResponse> modifyChannelSubscription(
        talon.gateway.Gateway.ModifyChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getModifyChannelSubscriptionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListChannelSubscriptionsResponse> listChannelSubscriptions(
        talon.gateway.Gateway.ListChannelSubscriptionsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListChannelSubscriptionsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteChannelSubscriptionResponse> deleteChannelSubscription(
        talon.gateway.Gateway.DeleteChannelSubscriptionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteChannelSubscriptionMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Schedules
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ScheduleResponse> createSchedule(
        talon.gateway.Gateway.CreateScheduleRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateScheduleMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ScheduleResponse> getSchedule(
        talon.gateway.Gateway.GetScheduleRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetScheduleMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ScheduleResponse> modifySchedule(
        talon.gateway.Gateway.ModifyScheduleRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getModifyScheduleMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListSchedulesResponse> listSchedules(
        talon.gateway.Gateway.ListSchedulesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListSchedulesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteScheduleResponse> deleteSchedule(
        talon.gateway.Gateway.DeleteScheduleRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteScheduleMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Namespaces
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.NamespaceResponse> createNamespace(
        talon.gateway.Gateway.CreateNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateNamespaceMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.NamespaceResponse> getNamespace(
        talon.gateway.Gateway.GetNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetNamespaceMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.NamespaceResponse> deleteNamespace(
        talon.gateway.Gateway.DeleteNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteNamespaceMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListNamespacesResponse> listNamespaces(
        talon.gateway.Gateway.ListNamespacesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListNamespacesMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Agent Templates
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.AgentTemplateResponse> createAgentTemplate(
        talon.gateway.Gateway.CreateAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateAgentTemplateMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.AgentTemplateResponse> getAgentTemplate(
        talon.gateway.Gateway.GetAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetAgentTemplateMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListAgentTemplatesResponse> listAgentTemplates(
        talon.gateway.Gateway.ListAgentTemplatesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListAgentTemplatesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteAgentTemplateResponse> deleteAgentTemplate(
        talon.gateway.Gateway.DeleteAgentTemplateRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteAgentTemplateMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * MCP Servers
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.McpServerResponse> createMcpServer(
        talon.gateway.Gateway.CreateMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateMcpServerMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.McpServerResponse> getMcpServer(
        talon.gateway.Gateway.GetMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMcpServerMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListMcpServersResponse> listMcpServers(
        talon.gateway.Gateway.ListMcpServersRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMcpServersMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteMcpServerResponse> deleteMcpServer(
        talon.gateway.Gateway.DeleteMcpServerRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteMcpServerMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.McpServerBindingResponse> createMcpServerBinding(
        talon.gateway.Gateway.CreateMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateMcpServerBindingMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.McpServerBindingResponse> getMcpServerBinding(
        talon.gateway.Gateway.GetMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMcpServerBindingMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListMcpServerBindingsResponse> listMcpServerBindings(
        talon.gateway.Gateway.ListMcpServerBindingsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMcpServerBindingsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteMcpServerBindingResponse> deleteMcpServerBinding(
        talon.gateway.Gateway.DeleteMcpServerBindingRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteMcpServerBindingMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE_AGENT = 0;
  private static final int METHODID_GET_AGENT = 1;
  private static final int METHODID_MODIFY_AGENT = 2;
  private static final int METHODID_LIST_AGENTS = 3;
  private static final int METHODID_GET_KNOWLEDGE = 4;
  private static final int METHODID_SEARCH_KNOWLEDGE = 5;
  private static final int METHODID_CREATE_NAMESPACE_KNOWLEDGE = 6;
  private static final int METHODID_GET_NAMESPACE_KNOWLEDGE = 7;
  private static final int METHODID_LIST_NAMESPACE_KNOWLEDGE = 8;
  private static final int METHODID_DELETE_NAMESPACE_KNOWLEDGE = 9;
  private static final int METHODID_CREATE_SESSION = 10;
  private static final int METHODID_GET_SESSION = 11;
  private static final int METHODID_LIST_SESSION_MESSAGES = 12;
  private static final int METHODID_LIST_SESSIONS = 13;
  private static final int METHODID_DELETE_SESSION = 14;
  private static final int METHODID_SEND_MESSAGE = 15;
  private static final int METHODID_APPEND_SESSION_MESSAGE = 16;
  private static final int METHODID_STOP_SESSION_GENERATION = 17;
  private static final int METHODID_STREAM_SESSION_PARTS = 18;
  private static final int METHODID_STREAM_SESSION_PARTS_BATCH = 19;
  private static final int METHODID_CREATE_CHANNEL = 20;
  private static final int METHODID_GET_CHANNEL = 21;
  private static final int METHODID_MODIFY_CHANNEL = 22;
  private static final int METHODID_LIST_CHANNELS = 23;
  private static final int METHODID_DELETE_CHANNEL = 24;
  private static final int METHODID_POST_CHANNEL_MESSAGE = 25;
  private static final int METHODID_GET_CHANNEL_MESSAGE = 26;
  private static final int METHODID_LIST_CHANNEL_MESSAGES = 27;
  private static final int METHODID_CREATE_CHANNEL_SUBSCRIPTION = 28;
  private static final int METHODID_GET_CHANNEL_SUBSCRIPTION = 29;
  private static final int METHODID_MODIFY_CHANNEL_SUBSCRIPTION = 30;
  private static final int METHODID_LIST_CHANNEL_SUBSCRIPTIONS = 31;
  private static final int METHODID_DELETE_CHANNEL_SUBSCRIPTION = 32;
  private static final int METHODID_STREAM_CHANNEL_EVENTS = 33;
  private static final int METHODID_CREATE_SCHEDULE = 34;
  private static final int METHODID_GET_SCHEDULE = 35;
  private static final int METHODID_MODIFY_SCHEDULE = 36;
  private static final int METHODID_LIST_SCHEDULES = 37;
  private static final int METHODID_DELETE_SCHEDULE = 38;
  private static final int METHODID_CREATE_NAMESPACE = 39;
  private static final int METHODID_GET_NAMESPACE = 40;
  private static final int METHODID_DELETE_NAMESPACE = 41;
  private static final int METHODID_LIST_NAMESPACES = 42;
  private static final int METHODID_CREATE_AGENT_TEMPLATE = 43;
  private static final int METHODID_GET_AGENT_TEMPLATE = 44;
  private static final int METHODID_LIST_AGENT_TEMPLATES = 45;
  private static final int METHODID_DELETE_AGENT_TEMPLATE = 46;
  private static final int METHODID_CREATE_MCP_SERVER = 47;
  private static final int METHODID_GET_MCP_SERVER = 48;
  private static final int METHODID_LIST_MCP_SERVERS = 49;
  private static final int METHODID_DELETE_MCP_SERVER = 50;
  private static final int METHODID_CREATE_MCP_SERVER_BINDING = 51;
  private static final int METHODID_GET_MCP_SERVER_BINDING = 52;
  private static final int METHODID_LIST_MCP_SERVER_BINDINGS = 53;
  private static final int METHODID_DELETE_MCP_SERVER_BINDING = 54;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final AsyncService serviceImpl;
    private final int methodId;

    MethodHandlers(AsyncService serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_CREATE_AGENT:
          serviceImpl.createAgent((talon.gateway.Gateway.CreateAgentRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse>) responseObserver);
          break;
        case METHODID_GET_AGENT:
          serviceImpl.getAgent((talon.gateway.Gateway.GetAgentRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.GetAgentResponse>) responseObserver);
          break;
        case METHODID_MODIFY_AGENT:
          serviceImpl.modifyAgent((talon.gateway.Gateway.ModifyAgentRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentResponse>) responseObserver);
          break;
        case METHODID_LIST_AGENTS:
          serviceImpl.listAgents((talon.gateway.Gateway.ListAgentsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentsResponse>) responseObserver);
          break;
        case METHODID_GET_KNOWLEDGE:
          serviceImpl.getKnowledge((talon.gateway.Gateway.GetKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.KnowledgeResponse>) responseObserver);
          break;
        case METHODID_SEARCH_KNOWLEDGE:
          serviceImpl.searchKnowledge((talon.gateway.Gateway.SearchKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.SearchKnowledgeResponse>) responseObserver);
          break;
        case METHODID_CREATE_NAMESPACE_KNOWLEDGE:
          serviceImpl.createNamespaceKnowledge((talon.gateway.Gateway.CreateNamespaceKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse>) responseObserver);
          break;
        case METHODID_GET_NAMESPACE_KNOWLEDGE:
          serviceImpl.getNamespaceKnowledge((talon.gateway.Gateway.GetNamespaceKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceKnowledgeResponse>) responseObserver);
          break;
        case METHODID_LIST_NAMESPACE_KNOWLEDGE:
          serviceImpl.listNamespaceKnowledge((talon.gateway.Gateway.ListNamespaceKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespaceKnowledgeResponse>) responseObserver);
          break;
        case METHODID_DELETE_NAMESPACE_KNOWLEDGE:
          serviceImpl.deleteNamespaceKnowledge((talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse>) responseObserver);
          break;
        case METHODID_CREATE_SESSION:
          serviceImpl.createSession((talon.gateway.Gateway.CreateSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse>) responseObserver);
          break;
        case METHODID_GET_SESSION:
          serviceImpl.getSession((talon.gateway.Gateway.GetSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.SessionResponse>) responseObserver);
          break;
        case METHODID_LIST_SESSION_MESSAGES:
          serviceImpl.listSessionMessages((talon.gateway.Gateway.ListSessionMessagesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionMessagesResponse>) responseObserver);
          break;
        case METHODID_LIST_SESSIONS:
          serviceImpl.listSessions((talon.gateway.Gateway.ListSessionsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSessionsResponse>) responseObserver);
          break;
        case METHODID_DELETE_SESSION:
          serviceImpl.deleteSession((talon.gateway.Gateway.DeleteSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteSessionResponse>) responseObserver);
          break;
        case METHODID_SEND_MESSAGE:
          serviceImpl.sendMessage((talon.gateway.Gateway.SendMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.SendMessageResponse>) responseObserver);
          break;
        case METHODID_APPEND_SESSION_MESSAGE:
          serviceImpl.appendSessionMessage((talon.gateway.Gateway.AppendSessionMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.AppendSessionMessageResponse>) responseObserver);
          break;
        case METHODID_STOP_SESSION_GENERATION:
          serviceImpl.stopSessionGeneration((talon.gateway.Gateway.StopSessionGenerationRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.StopSessionGenerationResponse>) responseObserver);
          break;
        case METHODID_STREAM_SESSION_PARTS:
          serviceImpl.streamSessionParts((talon.gateway.Gateway.StreamSessionPartsRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent>) responseObserver);
          break;
        case METHODID_STREAM_SESSION_PARTS_BATCH:
          serviceImpl.streamSessionPartsBatch((talon.gateway.Gateway.StreamSessionPartsBatchRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent>) responseObserver);
          break;
        case METHODID_CREATE_CHANNEL:
          serviceImpl.createChannel((talon.gateway.Gateway.CreateChannelRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse>) responseObserver);
          break;
        case METHODID_GET_CHANNEL:
          serviceImpl.getChannel((talon.gateway.Gateway.GetChannelRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse>) responseObserver);
          break;
        case METHODID_MODIFY_CHANNEL:
          serviceImpl.modifyChannel((talon.gateway.Gateway.ModifyChannelRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelResponse>) responseObserver);
          break;
        case METHODID_LIST_CHANNELS:
          serviceImpl.listChannels((talon.gateway.Gateway.ListChannelsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelsResponse>) responseObserver);
          break;
        case METHODID_DELETE_CHANNEL:
          serviceImpl.deleteChannel((talon.gateway.Gateway.DeleteChannelRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelResponse>) responseObserver);
          break;
        case METHODID_POST_CHANNEL_MESSAGE:
          serviceImpl.postChannelMessage((talon.gateway.Gateway.PostChannelMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.PostChannelMessageResponse>) responseObserver);
          break;
        case METHODID_GET_CHANNEL_MESSAGE:
          serviceImpl.getChannelMessage((talon.gateway.Gateway.GetChannelMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelMessageResponse>) responseObserver);
          break;
        case METHODID_LIST_CHANNEL_MESSAGES:
          serviceImpl.listChannelMessages((talon.gateway.Gateway.ListChannelMessagesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelMessagesResponse>) responseObserver);
          break;
        case METHODID_CREATE_CHANNEL_SUBSCRIPTION:
          serviceImpl.createChannelSubscription((talon.gateway.Gateway.CreateChannelSubscriptionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse>) responseObserver);
          break;
        case METHODID_GET_CHANNEL_SUBSCRIPTION:
          serviceImpl.getChannelSubscription((talon.gateway.Gateway.GetChannelSubscriptionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse>) responseObserver);
          break;
        case METHODID_MODIFY_CHANNEL_SUBSCRIPTION:
          serviceImpl.modifyChannelSubscription((talon.gateway.Gateway.ModifyChannelSubscriptionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ChannelSubscriptionResponse>) responseObserver);
          break;
        case METHODID_LIST_CHANNEL_SUBSCRIPTIONS:
          serviceImpl.listChannelSubscriptions((talon.gateway.Gateway.ListChannelSubscriptionsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListChannelSubscriptionsResponse>) responseObserver);
          break;
        case METHODID_DELETE_CHANNEL_SUBSCRIPTION:
          serviceImpl.deleteChannelSubscription((talon.gateway.Gateway.DeleteChannelSubscriptionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteChannelSubscriptionResponse>) responseObserver);
          break;
        case METHODID_STREAM_CHANNEL_EVENTS:
          serviceImpl.streamChannelEvents((talon.gateway.Gateway.StreamChannelEventsRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent>) responseObserver);
          break;
        case METHODID_CREATE_SCHEDULE:
          serviceImpl.createSchedule((talon.gateway.Gateway.CreateScheduleRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse>) responseObserver);
          break;
        case METHODID_GET_SCHEDULE:
          serviceImpl.getSchedule((talon.gateway.Gateway.GetScheduleRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse>) responseObserver);
          break;
        case METHODID_MODIFY_SCHEDULE:
          serviceImpl.modifySchedule((talon.gateway.Gateway.ModifyScheduleRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ScheduleResponse>) responseObserver);
          break;
        case METHODID_LIST_SCHEDULES:
          serviceImpl.listSchedules((talon.gateway.Gateway.ListSchedulesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListSchedulesResponse>) responseObserver);
          break;
        case METHODID_DELETE_SCHEDULE:
          serviceImpl.deleteSchedule((talon.gateway.Gateway.DeleteScheduleRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteScheduleResponse>) responseObserver);
          break;
        case METHODID_CREATE_NAMESPACE:
          serviceImpl.createNamespace((talon.gateway.Gateway.CreateNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse>) responseObserver);
          break;
        case METHODID_GET_NAMESPACE:
          serviceImpl.getNamespace((talon.gateway.Gateway.GetNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse>) responseObserver);
          break;
        case METHODID_DELETE_NAMESPACE:
          serviceImpl.deleteNamespace((talon.gateway.Gateway.DeleteNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.NamespaceResponse>) responseObserver);
          break;
        case METHODID_LIST_NAMESPACES:
          serviceImpl.listNamespaces((talon.gateway.Gateway.ListNamespacesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListNamespacesResponse>) responseObserver);
          break;
        case METHODID_CREATE_AGENT_TEMPLATE:
          serviceImpl.createAgentTemplate((talon.gateway.Gateway.CreateAgentTemplateRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse>) responseObserver);
          break;
        case METHODID_GET_AGENT_TEMPLATE:
          serviceImpl.getAgentTemplate((talon.gateway.Gateway.GetAgentTemplateRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.AgentTemplateResponse>) responseObserver);
          break;
        case METHODID_LIST_AGENT_TEMPLATES:
          serviceImpl.listAgentTemplates((talon.gateway.Gateway.ListAgentTemplatesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListAgentTemplatesResponse>) responseObserver);
          break;
        case METHODID_DELETE_AGENT_TEMPLATE:
          serviceImpl.deleteAgentTemplate((talon.gateway.Gateway.DeleteAgentTemplateRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteAgentTemplateResponse>) responseObserver);
          break;
        case METHODID_CREATE_MCP_SERVER:
          serviceImpl.createMcpServer((talon.gateway.Gateway.CreateMcpServerRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse>) responseObserver);
          break;
        case METHODID_GET_MCP_SERVER:
          serviceImpl.getMcpServer((talon.gateway.Gateway.GetMcpServerRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerResponse>) responseObserver);
          break;
        case METHODID_LIST_MCP_SERVERS:
          serviceImpl.listMcpServers((talon.gateway.Gateway.ListMcpServersRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServersResponse>) responseObserver);
          break;
        case METHODID_DELETE_MCP_SERVER:
          serviceImpl.deleteMcpServer((talon.gateway.Gateway.DeleteMcpServerRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerResponse>) responseObserver);
          break;
        case METHODID_CREATE_MCP_SERVER_BINDING:
          serviceImpl.createMcpServerBinding((talon.gateway.Gateway.CreateMcpServerBindingRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse>) responseObserver);
          break;
        case METHODID_GET_MCP_SERVER_BINDING:
          serviceImpl.getMcpServerBinding((talon.gateway.Gateway.GetMcpServerBindingRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.McpServerBindingResponse>) responseObserver);
          break;
        case METHODID_LIST_MCP_SERVER_BINDINGS:
          serviceImpl.listMcpServerBindings((talon.gateway.Gateway.ListMcpServerBindingsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListMcpServerBindingsResponse>) responseObserver);
          break;
        case METHODID_DELETE_MCP_SERVER_BINDING:
          serviceImpl.deleteMcpServerBinding((talon.gateway.Gateway.DeleteMcpServerBindingRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteMcpServerBindingResponse>) responseObserver);
          break;
        default:
          throw new AssertionError();
      }
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public io.grpc.stub.StreamObserver<Req> invoke(
        io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        default:
          throw new AssertionError();
      }
    }
  }

  public static final io.grpc.ServerServiceDefinition bindService(AsyncService service) {
    return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
        .addMethod(
          getCreateAgentMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateAgentRequest,
              talon.gateway.Gateway.AgentResponse>(
                service, METHODID_CREATE_AGENT)))
        .addMethod(
          getGetAgentMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetAgentRequest,
              talon.gateway.Gateway.GetAgentResponse>(
                service, METHODID_GET_AGENT)))
        .addMethod(
          getModifyAgentMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ModifyAgentRequest,
              talon.gateway.Gateway.AgentResponse>(
                service, METHODID_MODIFY_AGENT)))
        .addMethod(
          getListAgentsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListAgentsRequest,
              talon.gateway.Gateway.ListAgentsResponse>(
                service, METHODID_LIST_AGENTS)))
        .addMethod(
          getGetKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetKnowledgeRequest,
              talon.gateway.Gateway.KnowledgeResponse>(
                service, METHODID_GET_KNOWLEDGE)))
        .addMethod(
          getSearchKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.SearchKnowledgeRequest,
              talon.gateway.Gateway.SearchKnowledgeResponse>(
                service, METHODID_SEARCH_KNOWLEDGE)))
        .addMethod(
          getCreateNamespaceKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateNamespaceKnowledgeRequest,
              talon.gateway.Gateway.NamespaceKnowledgeResponse>(
                service, METHODID_CREATE_NAMESPACE_KNOWLEDGE)))
        .addMethod(
          getGetNamespaceKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetNamespaceKnowledgeRequest,
              talon.gateway.Gateway.NamespaceKnowledgeResponse>(
                service, METHODID_GET_NAMESPACE_KNOWLEDGE)))
        .addMethod(
          getListNamespaceKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListNamespaceKnowledgeRequest,
              talon.gateway.Gateway.ListNamespaceKnowledgeResponse>(
                service, METHODID_LIST_NAMESPACE_KNOWLEDGE)))
        .addMethod(
          getDeleteNamespaceKnowledgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteNamespaceKnowledgeRequest,
              talon.gateway.Gateway.DeleteNamespaceKnowledgeResponse>(
                service, METHODID_DELETE_NAMESPACE_KNOWLEDGE)))
        .addMethod(
          getCreateSessionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateSessionRequest,
              talon.gateway.Gateway.SessionResponse>(
                service, METHODID_CREATE_SESSION)))
        .addMethod(
          getGetSessionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetSessionRequest,
              talon.gateway.Gateway.SessionResponse>(
                service, METHODID_GET_SESSION)))
        .addMethod(
          getListSessionMessagesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListSessionMessagesRequest,
              talon.gateway.Gateway.ListSessionMessagesResponse>(
                service, METHODID_LIST_SESSION_MESSAGES)))
        .addMethod(
          getListSessionsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListSessionsRequest,
              talon.gateway.Gateway.ListSessionsResponse>(
                service, METHODID_LIST_SESSIONS)))
        .addMethod(
          getDeleteSessionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteSessionRequest,
              talon.gateway.Gateway.DeleteSessionResponse>(
                service, METHODID_DELETE_SESSION)))
        .addMethod(
          getSendMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.SendMessageRequest,
              talon.gateway.Gateway.SendMessageResponse>(
                service, METHODID_SEND_MESSAGE)))
        .addMethod(
          getAppendSessionMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.AppendSessionMessageRequest,
              talon.gateway.Gateway.AppendSessionMessageResponse>(
                service, METHODID_APPEND_SESSION_MESSAGE)))
        .addMethod(
          getStopSessionGenerationMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.StopSessionGenerationRequest,
              talon.gateway.Gateway.StopSessionGenerationResponse>(
                service, METHODID_STOP_SESSION_GENERATION)))
        .addMethod(
          getStreamSessionPartsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.gateway.Gateway.StreamSessionPartsRequest,
              talon.events.Events.SessionMessagePartEvent>(
                service, METHODID_STREAM_SESSION_PARTS)))
        .addMethod(
          getStreamSessionPartsBatchMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.gateway.Gateway.StreamSessionPartsBatchRequest,
              talon.events.Events.SessionMessagePartEvent>(
                service, METHODID_STREAM_SESSION_PARTS_BATCH)))
        .addMethod(
          getCreateChannelMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateChannelRequest,
              talon.gateway.Gateway.ChannelResponse>(
                service, METHODID_CREATE_CHANNEL)))
        .addMethod(
          getGetChannelMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetChannelRequest,
              talon.gateway.Gateway.ChannelResponse>(
                service, METHODID_GET_CHANNEL)))
        .addMethod(
          getModifyChannelMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ModifyChannelRequest,
              talon.gateway.Gateway.ChannelResponse>(
                service, METHODID_MODIFY_CHANNEL)))
        .addMethod(
          getListChannelsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListChannelsRequest,
              talon.gateway.Gateway.ListChannelsResponse>(
                service, METHODID_LIST_CHANNELS)))
        .addMethod(
          getDeleteChannelMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteChannelRequest,
              talon.gateway.Gateway.DeleteChannelResponse>(
                service, METHODID_DELETE_CHANNEL)))
        .addMethod(
          getPostChannelMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.PostChannelMessageRequest,
              talon.gateway.Gateway.PostChannelMessageResponse>(
                service, METHODID_POST_CHANNEL_MESSAGE)))
        .addMethod(
          getGetChannelMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetChannelMessageRequest,
              talon.gateway.Gateway.ChannelMessageResponse>(
                service, METHODID_GET_CHANNEL_MESSAGE)))
        .addMethod(
          getListChannelMessagesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListChannelMessagesRequest,
              talon.gateway.Gateway.ListChannelMessagesResponse>(
                service, METHODID_LIST_CHANNEL_MESSAGES)))
        .addMethod(
          getCreateChannelSubscriptionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateChannelSubscriptionRequest,
              talon.gateway.Gateway.ChannelSubscriptionResponse>(
                service, METHODID_CREATE_CHANNEL_SUBSCRIPTION)))
        .addMethod(
          getGetChannelSubscriptionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetChannelSubscriptionRequest,
              talon.gateway.Gateway.ChannelSubscriptionResponse>(
                service, METHODID_GET_CHANNEL_SUBSCRIPTION)))
        .addMethod(
          getModifyChannelSubscriptionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ModifyChannelSubscriptionRequest,
              talon.gateway.Gateway.ChannelSubscriptionResponse>(
                service, METHODID_MODIFY_CHANNEL_SUBSCRIPTION)))
        .addMethod(
          getListChannelSubscriptionsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListChannelSubscriptionsRequest,
              talon.gateway.Gateway.ListChannelSubscriptionsResponse>(
                service, METHODID_LIST_CHANNEL_SUBSCRIPTIONS)))
        .addMethod(
          getDeleteChannelSubscriptionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteChannelSubscriptionRequest,
              talon.gateway.Gateway.DeleteChannelSubscriptionResponse>(
                service, METHODID_DELETE_CHANNEL_SUBSCRIPTION)))
        .addMethod(
          getStreamChannelEventsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.gateway.Gateway.StreamChannelEventsRequest,
              talon.events.Events.ChannelEvent>(
                service, METHODID_STREAM_CHANNEL_EVENTS)))
        .addMethod(
          getCreateScheduleMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateScheduleRequest,
              talon.gateway.Gateway.ScheduleResponse>(
                service, METHODID_CREATE_SCHEDULE)))
        .addMethod(
          getGetScheduleMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetScheduleRequest,
              talon.gateway.Gateway.ScheduleResponse>(
                service, METHODID_GET_SCHEDULE)))
        .addMethod(
          getModifyScheduleMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ModifyScheduleRequest,
              talon.gateway.Gateway.ScheduleResponse>(
                service, METHODID_MODIFY_SCHEDULE)))
        .addMethod(
          getListSchedulesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListSchedulesRequest,
              talon.gateway.Gateway.ListSchedulesResponse>(
                service, METHODID_LIST_SCHEDULES)))
        .addMethod(
          getDeleteScheduleMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteScheduleRequest,
              talon.gateway.Gateway.DeleteScheduleResponse>(
                service, METHODID_DELETE_SCHEDULE)))
        .addMethod(
          getCreateNamespaceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateNamespaceRequest,
              talon.gateway.Gateway.NamespaceResponse>(
                service, METHODID_CREATE_NAMESPACE)))
        .addMethod(
          getGetNamespaceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetNamespaceRequest,
              talon.gateway.Gateway.NamespaceResponse>(
                service, METHODID_GET_NAMESPACE)))
        .addMethod(
          getDeleteNamespaceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteNamespaceRequest,
              talon.gateway.Gateway.NamespaceResponse>(
                service, METHODID_DELETE_NAMESPACE)))
        .addMethod(
          getListNamespacesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListNamespacesRequest,
              talon.gateway.Gateway.ListNamespacesResponse>(
                service, METHODID_LIST_NAMESPACES)))
        .addMethod(
          getCreateAgentTemplateMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateAgentTemplateRequest,
              talon.gateway.Gateway.AgentTemplateResponse>(
                service, METHODID_CREATE_AGENT_TEMPLATE)))
        .addMethod(
          getGetAgentTemplateMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetAgentTemplateRequest,
              talon.gateway.Gateway.AgentTemplateResponse>(
                service, METHODID_GET_AGENT_TEMPLATE)))
        .addMethod(
          getListAgentTemplatesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListAgentTemplatesRequest,
              talon.gateway.Gateway.ListAgentTemplatesResponse>(
                service, METHODID_LIST_AGENT_TEMPLATES)))
        .addMethod(
          getDeleteAgentTemplateMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteAgentTemplateRequest,
              talon.gateway.Gateway.DeleteAgentTemplateResponse>(
                service, METHODID_DELETE_AGENT_TEMPLATE)))
        .addMethod(
          getCreateMcpServerMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateMcpServerRequest,
              talon.gateway.Gateway.McpServerResponse>(
                service, METHODID_CREATE_MCP_SERVER)))
        .addMethod(
          getGetMcpServerMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetMcpServerRequest,
              talon.gateway.Gateway.McpServerResponse>(
                service, METHODID_GET_MCP_SERVER)))
        .addMethod(
          getListMcpServersMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListMcpServersRequest,
              talon.gateway.Gateway.ListMcpServersResponse>(
                service, METHODID_LIST_MCP_SERVERS)))
        .addMethod(
          getDeleteMcpServerMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteMcpServerRequest,
              talon.gateway.Gateway.DeleteMcpServerResponse>(
                service, METHODID_DELETE_MCP_SERVER)))
        .addMethod(
          getCreateMcpServerBindingMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateMcpServerBindingRequest,
              talon.gateway.Gateway.McpServerBindingResponse>(
                service, METHODID_CREATE_MCP_SERVER_BINDING)))
        .addMethod(
          getGetMcpServerBindingMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetMcpServerBindingRequest,
              talon.gateway.Gateway.McpServerBindingResponse>(
                service, METHODID_GET_MCP_SERVER_BINDING)))
        .addMethod(
          getListMcpServerBindingsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListMcpServerBindingsRequest,
              talon.gateway.Gateway.ListMcpServerBindingsResponse>(
                service, METHODID_LIST_MCP_SERVER_BINDINGS)))
        .addMethod(
          getDeleteMcpServerBindingMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteMcpServerBindingRequest,
              talon.gateway.Gateway.DeleteMcpServerBindingResponse>(
                service, METHODID_DELETE_MCP_SERVER_BINDING)))
        .build();
  }

  private static abstract class GatewayServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    GatewayServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.gateway.Gateway.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("GatewayService");
    }
  }

  private static final class GatewayServiceFileDescriptorSupplier
      extends GatewayServiceBaseDescriptorSupplier {
    GatewayServiceFileDescriptorSupplier() {}
  }

  private static final class GatewayServiceMethodDescriptorSupplier
      extends GatewayServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    GatewayServiceMethodDescriptorSupplier(java.lang.String methodName) {
      this.methodName = methodName;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.MethodDescriptor getMethodDescriptor() {
      return getServiceDescriptor().findMethodByName(methodName);
    }
  }

  private static volatile io.grpc.ServiceDescriptor serviceDescriptor;

  public static io.grpc.ServiceDescriptor getServiceDescriptor() {
    io.grpc.ServiceDescriptor result = serviceDescriptor;
    if (result == null) {
      synchronized (GatewayServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new GatewayServiceFileDescriptorSupplier())
              .addMethod(getCreateAgentMethod())
              .addMethod(getGetAgentMethod())
              .addMethod(getModifyAgentMethod())
              .addMethod(getListAgentsMethod())
              .addMethod(getGetKnowledgeMethod())
              .addMethod(getSearchKnowledgeMethod())
              .addMethod(getCreateNamespaceKnowledgeMethod())
              .addMethod(getGetNamespaceKnowledgeMethod())
              .addMethod(getListNamespaceKnowledgeMethod())
              .addMethod(getDeleteNamespaceKnowledgeMethod())
              .addMethod(getCreateSessionMethod())
              .addMethod(getGetSessionMethod())
              .addMethod(getListSessionMessagesMethod())
              .addMethod(getListSessionsMethod())
              .addMethod(getDeleteSessionMethod())
              .addMethod(getSendMessageMethod())
              .addMethod(getAppendSessionMessageMethod())
              .addMethod(getStopSessionGenerationMethod())
              .addMethod(getStreamSessionPartsMethod())
              .addMethod(getStreamSessionPartsBatchMethod())
              .addMethod(getCreateChannelMethod())
              .addMethod(getGetChannelMethod())
              .addMethod(getModifyChannelMethod())
              .addMethod(getListChannelsMethod())
              .addMethod(getDeleteChannelMethod())
              .addMethod(getPostChannelMessageMethod())
              .addMethod(getGetChannelMessageMethod())
              .addMethod(getListChannelMessagesMethod())
              .addMethod(getCreateChannelSubscriptionMethod())
              .addMethod(getGetChannelSubscriptionMethod())
              .addMethod(getModifyChannelSubscriptionMethod())
              .addMethod(getListChannelSubscriptionsMethod())
              .addMethod(getDeleteChannelSubscriptionMethod())
              .addMethod(getStreamChannelEventsMethod())
              .addMethod(getCreateScheduleMethod())
              .addMethod(getGetScheduleMethod())
              .addMethod(getModifyScheduleMethod())
              .addMethod(getListSchedulesMethod())
              .addMethod(getDeleteScheduleMethod())
              .addMethod(getCreateNamespaceMethod())
              .addMethod(getGetNamespaceMethod())
              .addMethod(getDeleteNamespaceMethod())
              .addMethod(getListNamespacesMethod())
              .addMethod(getCreateAgentTemplateMethod())
              .addMethod(getGetAgentTemplateMethod())
              .addMethod(getListAgentTemplatesMethod())
              .addMethod(getDeleteAgentTemplateMethod())
              .addMethod(getCreateMcpServerMethod())
              .addMethod(getGetMcpServerMethod())
              .addMethod(getListMcpServersMethod())
              .addMethod(getDeleteMcpServerMethod())
              .addMethod(getCreateMcpServerBindingMethod())
              .addMethod(getGetMcpServerBindingMethod())
              .addMethod(getListMcpServerBindingsMethod())
              .addMethod(getDeleteMcpServerBindingMethod())
              .build();
        }
      }
    }
    return result;
  }
}
