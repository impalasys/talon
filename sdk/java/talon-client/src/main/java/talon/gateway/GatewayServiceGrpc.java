package talon.gateway;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class GatewayServiceGrpc {

  private GatewayServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.gateway.GatewayService";

  // Static method descriptors that strictly reflect the proto.
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

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ClearSessionRequest,
      talon.gateway.Gateway.ClearSessionResponse> getClearSessionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ClearSession",
      requestType = talon.gateway.Gateway.ClearSessionRequest.class,
      responseType = talon.gateway.Gateway.ClearSessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ClearSessionRequest,
      talon.gateway.Gateway.ClearSessionResponse> getClearSessionMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ClearSessionRequest, talon.gateway.Gateway.ClearSessionResponse> getClearSessionMethod;
    if ((getClearSessionMethod = GatewayServiceGrpc.getClearSessionMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getClearSessionMethod = GatewayServiceGrpc.getClearSessionMethod) == null) {
          GatewayServiceGrpc.getClearSessionMethod = getClearSessionMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ClearSessionRequest, talon.gateway.Gateway.ClearSessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ClearSession"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ClearSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ClearSessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ClearSession"))
              .build();
        }
      }
    }
    return getClearSessionMethod;
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

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getCreateWorkflowRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateWorkflowRun",
      requestType = talon.gateway.Gateway.CreateWorkflowRunRequest.class,
      responseType = talon.gateway.Gateway.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getCreateWorkflowRunMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse> getCreateWorkflowRunMethod;
    if ((getCreateWorkflowRunMethod = GatewayServiceGrpc.getCreateWorkflowRunMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateWorkflowRunMethod = GatewayServiceGrpc.getCreateWorkflowRunMethod) == null) {
          GatewayServiceGrpc.getCreateWorkflowRunMethod = getCreateWorkflowRunMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateWorkflowRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateWorkflowRun"))
              .build();
        }
      }
    }
    return getCreateWorkflowRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getGetWorkflowRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetWorkflowRun",
      requestType = talon.gateway.Gateway.GetWorkflowRunRequest.class,
      responseType = talon.gateway.Gateway.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getGetWorkflowRunMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse> getGetWorkflowRunMethod;
    if ((getGetWorkflowRunMethod = GatewayServiceGrpc.getGetWorkflowRunMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetWorkflowRunMethod = GatewayServiceGrpc.getGetWorkflowRunMethod) == null) {
          GatewayServiceGrpc.getGetWorkflowRunMethod = getGetWorkflowRunMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetWorkflowRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetWorkflowRun"))
              .build();
        }
      }
    }
    return getGetWorkflowRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListWorkflowRunsRequest,
      talon.gateway.Gateway.ListWorkflowRunsResponse> getListWorkflowRunsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListWorkflowRuns",
      requestType = talon.gateway.Gateway.ListWorkflowRunsRequest.class,
      responseType = talon.gateway.Gateway.ListWorkflowRunsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListWorkflowRunsRequest,
      talon.gateway.Gateway.ListWorkflowRunsResponse> getListWorkflowRunsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListWorkflowRunsRequest, talon.gateway.Gateway.ListWorkflowRunsResponse> getListWorkflowRunsMethod;
    if ((getListWorkflowRunsMethod = GatewayServiceGrpc.getListWorkflowRunsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListWorkflowRunsMethod = GatewayServiceGrpc.getListWorkflowRunsMethod) == null) {
          GatewayServiceGrpc.getListWorkflowRunsMethod = getListWorkflowRunsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListWorkflowRunsRequest, talon.gateway.Gateway.ListWorkflowRunsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListWorkflowRuns"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListWorkflowRunsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListWorkflowRunsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListWorkflowRuns"))
              .build();
        }
      }
    }
    return getListWorkflowRunsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ResumeWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getResumeWorkflowRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ResumeWorkflowRun",
      requestType = talon.gateway.Gateway.ResumeWorkflowRunRequest.class,
      responseType = talon.gateway.Gateway.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ResumeWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getResumeWorkflowRunMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ResumeWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse> getResumeWorkflowRunMethod;
    if ((getResumeWorkflowRunMethod = GatewayServiceGrpc.getResumeWorkflowRunMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getResumeWorkflowRunMethod = GatewayServiceGrpc.getResumeWorkflowRunMethod) == null) {
          GatewayServiceGrpc.getResumeWorkflowRunMethod = getResumeWorkflowRunMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ResumeWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ResumeWorkflowRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ResumeWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ResumeWorkflowRun"))
              .build();
        }
      }
    }
    return getResumeWorkflowRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CancelWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getCancelWorkflowRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CancelWorkflowRun",
      requestType = talon.gateway.Gateway.CancelWorkflowRunRequest.class,
      responseType = talon.gateway.Gateway.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CancelWorkflowRunRequest,
      talon.gateway.Gateway.WorkflowRunResponse> getCancelWorkflowRunMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CancelWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse> getCancelWorkflowRunMethod;
    if ((getCancelWorkflowRunMethod = GatewayServiceGrpc.getCancelWorkflowRunMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCancelWorkflowRunMethod = GatewayServiceGrpc.getCancelWorkflowRunMethod) == null) {
          GatewayServiceGrpc.getCancelWorkflowRunMethod = getCancelWorkflowRunMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CancelWorkflowRunRequest, talon.gateway.Gateway.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CancelWorkflowRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CancelWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CancelWorkflowRun"))
              .build();
        }
      }
    }
    return getCancelWorkflowRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamWorkflowEventsRequest,
      talon.data.Data.WorkflowRunEvent> getStreamWorkflowEventsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamWorkflowEvents",
      requestType = talon.gateway.Gateway.StreamWorkflowEventsRequest.class,
      responseType = talon.data.Data.WorkflowRunEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamWorkflowEventsRequest,
      talon.data.Data.WorkflowRunEvent> getStreamWorkflowEventsMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.StreamWorkflowEventsRequest, talon.data.Data.WorkflowRunEvent> getStreamWorkflowEventsMethod;
    if ((getStreamWorkflowEventsMethod = GatewayServiceGrpc.getStreamWorkflowEventsMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getStreamWorkflowEventsMethod = GatewayServiceGrpc.getStreamWorkflowEventsMethod) == null) {
          GatewayServiceGrpc.getStreamWorkflowEventsMethod = getStreamWorkflowEventsMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.StreamWorkflowEventsRequest, talon.data.Data.WorkflowRunEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamWorkflowEvents"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.StreamWorkflowEventsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.data.Data.WorkflowRunEvent.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("StreamWorkflowEvents"))
              .build();
        }
      }
    }
    return getStreamWorkflowEventsMethod;
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

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateResourceRequest,
      talon.gateway.Gateway.ResourceResponse> getCreateResourceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateResource",
      requestType = talon.gateway.Gateway.CreateResourceRequest.class,
      responseType = talon.gateway.Gateway.ResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateResourceRequest,
      talon.gateway.Gateway.ResourceResponse> getCreateResourceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.CreateResourceRequest, talon.gateway.Gateway.ResourceResponse> getCreateResourceMethod;
    if ((getCreateResourceMethod = GatewayServiceGrpc.getCreateResourceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getCreateResourceMethod = GatewayServiceGrpc.getCreateResourceMethod) == null) {
          GatewayServiceGrpc.getCreateResourceMethod = getCreateResourceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.CreateResourceRequest, talon.gateway.Gateway.ResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateResource"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.CreateResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("CreateResource"))
              .build();
        }
      }
    }
    return getCreateResourceMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.GetResourceRequest,
      talon.gateway.Gateway.ResourceResponse> getGetResourceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetResource",
      requestType = talon.gateway.Gateway.GetResourceRequest.class,
      responseType = talon.gateway.Gateway.ResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.GetResourceRequest,
      talon.gateway.Gateway.ResourceResponse> getGetResourceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.GetResourceRequest, talon.gateway.Gateway.ResourceResponse> getGetResourceMethod;
    if ((getGetResourceMethod = GatewayServiceGrpc.getGetResourceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getGetResourceMethod = GatewayServiceGrpc.getGetResourceMethod) == null) {
          GatewayServiceGrpc.getGetResourceMethod = getGetResourceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.GetResourceRequest, talon.gateway.Gateway.ResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetResource"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.GetResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("GetResource"))
              .build();
        }
      }
    }
    return getGetResourceMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.ListResourcesRequest,
      talon.gateway.Gateway.ListResourcesResponse> getListResourcesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListResources",
      requestType = talon.gateway.Gateway.ListResourcesRequest.class,
      responseType = talon.gateway.Gateway.ListResourcesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.ListResourcesRequest,
      talon.gateway.Gateway.ListResourcesResponse> getListResourcesMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.ListResourcesRequest, talon.gateway.Gateway.ListResourcesResponse> getListResourcesMethod;
    if ((getListResourcesMethod = GatewayServiceGrpc.getListResourcesMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getListResourcesMethod = GatewayServiceGrpc.getListResourcesMethod) == null) {
          GatewayServiceGrpc.getListResourcesMethod = getListResourcesMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.ListResourcesRequest, talon.gateway.Gateway.ListResourcesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListResources"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListResourcesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.ListResourcesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("ListResources"))
              .build();
        }
      }
    }
    return getListResourcesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteResourceRequest,
      talon.gateway.Gateway.DeleteResourceResponse> getDeleteResourceMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteResource",
      requestType = talon.gateway.Gateway.DeleteResourceRequest.class,
      responseType = talon.gateway.Gateway.DeleteResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteResourceRequest,
      talon.gateway.Gateway.DeleteResourceResponse> getDeleteResourceMethod() {
    io.grpc.MethodDescriptor<talon.gateway.Gateway.DeleteResourceRequest, talon.gateway.Gateway.DeleteResourceResponse> getDeleteResourceMethod;
    if ((getDeleteResourceMethod = GatewayServiceGrpc.getDeleteResourceMethod) == null) {
      synchronized (GatewayServiceGrpc.class) {
        if ((getDeleteResourceMethod = GatewayServiceGrpc.getDeleteResourceMethod) == null) {
          GatewayServiceGrpc.getDeleteResourceMethod = getDeleteResourceMethod =
              io.grpc.MethodDescriptor.<talon.gateway.Gateway.DeleteResourceRequest, talon.gateway.Gateway.DeleteResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteResource"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.gateway.Gateway.DeleteResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new GatewayServiceMethodDescriptorSupplier("DeleteResource"))
              .build();
        }
      }
    }
    return getDeleteResourceMethod;
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
     * Agent knowledge data-plane queries
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
     */
    default void clearSession(talon.gateway.Gateway.ClearSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ClearSessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getClearSessionMethod(), responseObserver);
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
     * Channel data-plane actions
     * </pre>
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
    default void streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamChannelEventsMethod(), responseObserver);
    }

    /**
     * <pre>
     * Workflow data-plane actions
     * </pre>
     */
    default void createWorkflowRun(talon.gateway.Gateway.CreateWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateWorkflowRunMethod(), responseObserver);
    }

    /**
     */
    default void getWorkflowRun(talon.gateway.Gateway.GetWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetWorkflowRunMethod(), responseObserver);
    }

    /**
     */
    default void listWorkflowRuns(talon.gateway.Gateway.ListWorkflowRunsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListWorkflowRunsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListWorkflowRunsMethod(), responseObserver);
    }

    /**
     */
    default void resumeWorkflowRun(talon.gateway.Gateway.ResumeWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getResumeWorkflowRunMethod(), responseObserver);
    }

    /**
     */
    default void cancelWorkflowRun(talon.gateway.Gateway.CancelWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCancelWorkflowRunMethod(), responseObserver);
    }

    /**
     */
    default void streamWorkflowEvents(talon.gateway.Gateway.StreamWorkflowEventsRequest request,
        io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamWorkflowEventsMethod(), responseObserver);
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
     * Generic v2 resources
     * </pre>
     */
    default void createResource(talon.gateway.Gateway.CreateResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateResourceMethod(), responseObserver);
    }

    /**
     */
    default void getResource(talon.gateway.Gateway.GetResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetResourceMethod(), responseObserver);
    }

    /**
     */
    default void listResources(talon.gateway.Gateway.ListResourcesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListResourcesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListResourcesMethod(), responseObserver);
    }

    /**
     */
    default void deleteResource(talon.gateway.Gateway.DeleteResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteResourceMethod(), responseObserver);
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
     * Agent knowledge data-plane queries
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
     */
    public void clearSession(talon.gateway.Gateway.ClearSessionRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ClearSessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getClearSessionMethod(), getCallOptions()), request, responseObserver);
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
     * Channel data-plane actions
     * </pre>
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
    public void streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamChannelEventsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Workflow data-plane actions
     * </pre>
     */
    public void createWorkflowRun(talon.gateway.Gateway.CreateWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateWorkflowRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getWorkflowRun(talon.gateway.Gateway.GetWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetWorkflowRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listWorkflowRuns(talon.gateway.Gateway.ListWorkflowRunsRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListWorkflowRunsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListWorkflowRunsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void resumeWorkflowRun(talon.gateway.Gateway.ResumeWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getResumeWorkflowRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void cancelWorkflowRun(talon.gateway.Gateway.CancelWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCancelWorkflowRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamWorkflowEvents(talon.gateway.Gateway.StreamWorkflowEventsRequest request,
        io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamWorkflowEventsMethod(), getCallOptions()), request, responseObserver);
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
     * Generic v2 resources
     * </pre>
     */
    public void createResource(talon.gateway.Gateway.CreateResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateResourceMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getResource(talon.gateway.Gateway.GetResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetResourceMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listResources(talon.gateway.Gateway.ListResourcesRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListResourcesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListResourcesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteResource(talon.gateway.Gateway.DeleteResourceRequest request,
        io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteResourceMethod(), getCallOptions()), request, responseObserver);
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
     * Agent knowledge data-plane queries
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
     */
    public talon.gateway.Gateway.ClearSessionResponse clearSession(talon.gateway.Gateway.ClearSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getClearSessionMethod(), getCallOptions(), request);
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
     * Channel data-plane actions
     * </pre>
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
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.ChannelEvent>
        streamChannelEvents(talon.gateway.Gateway.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamChannelEventsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Workflow data-plane actions
     * </pre>
     */
    public talon.gateway.Gateway.WorkflowRunResponse createWorkflowRun(talon.gateway.Gateway.CreateWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse getWorkflowRun(talon.gateway.Gateway.GetWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListWorkflowRunsResponse listWorkflowRuns(talon.gateway.Gateway.ListWorkflowRunsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListWorkflowRunsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse resumeWorkflowRun(talon.gateway.Gateway.ResumeWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getResumeWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse cancelWorkflowRun(talon.gateway.Gateway.CancelWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCancelWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.data.Data.WorkflowRunEvent>
        streamWorkflowEvents(talon.gateway.Gateway.StreamWorkflowEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamWorkflowEventsMethod(), getCallOptions(), request);
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
     * Generic v2 resources
     * </pre>
     */
    public talon.gateway.Gateway.ResourceResponse createResource(talon.gateway.Gateway.CreateResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateResourceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ResourceResponse getResource(talon.gateway.Gateway.GetResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetResourceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListResourcesResponse listResources(talon.gateway.Gateway.ListResourcesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListResourcesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteResourceResponse deleteResource(talon.gateway.Gateway.DeleteResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteResourceMethod(), getCallOptions(), request);
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
     * Agent knowledge data-plane queries
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
     */
    public talon.gateway.Gateway.ClearSessionResponse clearSession(talon.gateway.Gateway.ClearSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getClearSessionMethod(), getCallOptions(), request);
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
     * Channel data-plane actions
     * </pre>
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
    public java.util.Iterator<talon.events.Events.ChannelEvent> streamChannelEvents(
        talon.gateway.Gateway.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamChannelEventsMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Workflow data-plane actions
     * </pre>
     */
    public talon.gateway.Gateway.WorkflowRunResponse createWorkflowRun(talon.gateway.Gateway.CreateWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse getWorkflowRun(talon.gateway.Gateway.GetWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListWorkflowRunsResponse listWorkflowRuns(talon.gateway.Gateway.ListWorkflowRunsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListWorkflowRunsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse resumeWorkflowRun(talon.gateway.Gateway.ResumeWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getResumeWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.WorkflowRunResponse cancelWorkflowRun(talon.gateway.Gateway.CancelWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCancelWorkflowRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.data.Data.WorkflowRunEvent> streamWorkflowEvents(
        talon.gateway.Gateway.StreamWorkflowEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamWorkflowEventsMethod(), getCallOptions(), request);
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
     * Generic v2 resources
     * </pre>
     */
    public talon.gateway.Gateway.ResourceResponse createResource(talon.gateway.Gateway.CreateResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateResourceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ResourceResponse getResource(talon.gateway.Gateway.GetResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetResourceMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.ListResourcesResponse listResources(talon.gateway.Gateway.ListResourcesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListResourcesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.gateway.Gateway.DeleteResourceResponse deleteResource(talon.gateway.Gateway.DeleteResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteResourceMethod(), getCallOptions(), request);
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
     * Agent knowledge data-plane queries
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
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ClearSessionResponse> clearSession(
        talon.gateway.Gateway.ClearSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getClearSessionMethod(), getCallOptions()), request);
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
     * Channel data-plane actions
     * </pre>
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
     * <pre>
     * Workflow data-plane actions
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.WorkflowRunResponse> createWorkflowRun(
        talon.gateway.Gateway.CreateWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateWorkflowRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.WorkflowRunResponse> getWorkflowRun(
        talon.gateway.Gateway.GetWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetWorkflowRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListWorkflowRunsResponse> listWorkflowRuns(
        talon.gateway.Gateway.ListWorkflowRunsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListWorkflowRunsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.WorkflowRunResponse> resumeWorkflowRun(
        talon.gateway.Gateway.ResumeWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getResumeWorkflowRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.WorkflowRunResponse> cancelWorkflowRun(
        talon.gateway.Gateway.CancelWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCancelWorkflowRunMethod(), getCallOptions()), request);
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
     * Generic v2 resources
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ResourceResponse> createResource(
        talon.gateway.Gateway.CreateResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateResourceMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ResourceResponse> getResource(
        talon.gateway.Gateway.GetResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetResourceMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.ListResourcesResponse> listResources(
        talon.gateway.Gateway.ListResourcesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListResourcesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.gateway.Gateway.DeleteResourceResponse> deleteResource(
        talon.gateway.Gateway.DeleteResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteResourceMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_GET_KNOWLEDGE = 0;
  private static final int METHODID_SEARCH_KNOWLEDGE = 1;
  private static final int METHODID_CREATE_SESSION = 2;
  private static final int METHODID_GET_SESSION = 3;
  private static final int METHODID_LIST_SESSION_MESSAGES = 4;
  private static final int METHODID_LIST_SESSIONS = 5;
  private static final int METHODID_DELETE_SESSION = 6;
  private static final int METHODID_CLEAR_SESSION = 7;
  private static final int METHODID_SEND_MESSAGE = 8;
  private static final int METHODID_APPEND_SESSION_MESSAGE = 9;
  private static final int METHODID_STOP_SESSION_GENERATION = 10;
  private static final int METHODID_STREAM_SESSION_PARTS = 11;
  private static final int METHODID_STREAM_SESSION_PARTS_BATCH = 12;
  private static final int METHODID_POST_CHANNEL_MESSAGE = 13;
  private static final int METHODID_GET_CHANNEL_MESSAGE = 14;
  private static final int METHODID_LIST_CHANNEL_MESSAGES = 15;
  private static final int METHODID_STREAM_CHANNEL_EVENTS = 16;
  private static final int METHODID_CREATE_WORKFLOW_RUN = 17;
  private static final int METHODID_GET_WORKFLOW_RUN = 18;
  private static final int METHODID_LIST_WORKFLOW_RUNS = 19;
  private static final int METHODID_RESUME_WORKFLOW_RUN = 20;
  private static final int METHODID_CANCEL_WORKFLOW_RUN = 21;
  private static final int METHODID_STREAM_WORKFLOW_EVENTS = 22;
  private static final int METHODID_CREATE_NAMESPACE = 23;
  private static final int METHODID_GET_NAMESPACE = 24;
  private static final int METHODID_DELETE_NAMESPACE = 25;
  private static final int METHODID_LIST_NAMESPACES = 26;
  private static final int METHODID_CREATE_RESOURCE = 27;
  private static final int METHODID_GET_RESOURCE = 28;
  private static final int METHODID_LIST_RESOURCES = 29;
  private static final int METHODID_DELETE_RESOURCE = 30;

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
        case METHODID_GET_KNOWLEDGE:
          serviceImpl.getKnowledge((talon.gateway.Gateway.GetKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.KnowledgeResponse>) responseObserver);
          break;
        case METHODID_SEARCH_KNOWLEDGE:
          serviceImpl.searchKnowledge((talon.gateway.Gateway.SearchKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.SearchKnowledgeResponse>) responseObserver);
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
        case METHODID_CLEAR_SESSION:
          serviceImpl.clearSession((talon.gateway.Gateway.ClearSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ClearSessionResponse>) responseObserver);
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
        case METHODID_STREAM_CHANNEL_EVENTS:
          serviceImpl.streamChannelEvents((talon.gateway.Gateway.StreamChannelEventsRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent>) responseObserver);
          break;
        case METHODID_CREATE_WORKFLOW_RUN:
          serviceImpl.createWorkflowRun((talon.gateway.Gateway.CreateWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_GET_WORKFLOW_RUN:
          serviceImpl.getWorkflowRun((talon.gateway.Gateway.GetWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_LIST_WORKFLOW_RUNS:
          serviceImpl.listWorkflowRuns((talon.gateway.Gateway.ListWorkflowRunsRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListWorkflowRunsResponse>) responseObserver);
          break;
        case METHODID_RESUME_WORKFLOW_RUN:
          serviceImpl.resumeWorkflowRun((talon.gateway.Gateway.ResumeWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_CANCEL_WORKFLOW_RUN:
          serviceImpl.cancelWorkflowRun((talon.gateway.Gateway.CancelWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_STREAM_WORKFLOW_EVENTS:
          serviceImpl.streamWorkflowEvents((talon.gateway.Gateway.StreamWorkflowEventsRequest) request,
              (io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent>) responseObserver);
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
        case METHODID_CREATE_RESOURCE:
          serviceImpl.createResource((talon.gateway.Gateway.CreateResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse>) responseObserver);
          break;
        case METHODID_GET_RESOURCE:
          serviceImpl.getResource((talon.gateway.Gateway.GetResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ResourceResponse>) responseObserver);
          break;
        case METHODID_LIST_RESOURCES:
          serviceImpl.listResources((talon.gateway.Gateway.ListResourcesRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.ListResourcesResponse>) responseObserver);
          break;
        case METHODID_DELETE_RESOURCE:
          serviceImpl.deleteResource((talon.gateway.Gateway.DeleteResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.gateway.Gateway.DeleteResourceResponse>) responseObserver);
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
          getClearSessionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ClearSessionRequest,
              talon.gateway.Gateway.ClearSessionResponse>(
                service, METHODID_CLEAR_SESSION)))
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
          getStreamChannelEventsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.gateway.Gateway.StreamChannelEventsRequest,
              talon.events.Events.ChannelEvent>(
                service, METHODID_STREAM_CHANNEL_EVENTS)))
        .addMethod(
          getCreateWorkflowRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateWorkflowRunRequest,
              talon.gateway.Gateway.WorkflowRunResponse>(
                service, METHODID_CREATE_WORKFLOW_RUN)))
        .addMethod(
          getGetWorkflowRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetWorkflowRunRequest,
              talon.gateway.Gateway.WorkflowRunResponse>(
                service, METHODID_GET_WORKFLOW_RUN)))
        .addMethod(
          getListWorkflowRunsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListWorkflowRunsRequest,
              talon.gateway.Gateway.ListWorkflowRunsResponse>(
                service, METHODID_LIST_WORKFLOW_RUNS)))
        .addMethod(
          getResumeWorkflowRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ResumeWorkflowRunRequest,
              talon.gateway.Gateway.WorkflowRunResponse>(
                service, METHODID_RESUME_WORKFLOW_RUN)))
        .addMethod(
          getCancelWorkflowRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CancelWorkflowRunRequest,
              talon.gateway.Gateway.WorkflowRunResponse>(
                service, METHODID_CANCEL_WORKFLOW_RUN)))
        .addMethod(
          getStreamWorkflowEventsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.gateway.Gateway.StreamWorkflowEventsRequest,
              talon.data.Data.WorkflowRunEvent>(
                service, METHODID_STREAM_WORKFLOW_EVENTS)))
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
          getCreateResourceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.CreateResourceRequest,
              talon.gateway.Gateway.ResourceResponse>(
                service, METHODID_CREATE_RESOURCE)))
        .addMethod(
          getGetResourceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.GetResourceRequest,
              talon.gateway.Gateway.ResourceResponse>(
                service, METHODID_GET_RESOURCE)))
        .addMethod(
          getListResourcesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.ListResourcesRequest,
              talon.gateway.Gateway.ListResourcesResponse>(
                service, METHODID_LIST_RESOURCES)))
        .addMethod(
          getDeleteResourceMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.gateway.Gateway.DeleteResourceRequest,
              talon.gateway.Gateway.DeleteResourceResponse>(
                service, METHODID_DELETE_RESOURCE)))
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
              .addMethod(getGetKnowledgeMethod())
              .addMethod(getSearchKnowledgeMethod())
              .addMethod(getCreateSessionMethod())
              .addMethod(getGetSessionMethod())
              .addMethod(getListSessionMessagesMethod())
              .addMethod(getListSessionsMethod())
              .addMethod(getDeleteSessionMethod())
              .addMethod(getClearSessionMethod())
              .addMethod(getSendMessageMethod())
              .addMethod(getAppendSessionMessageMethod())
              .addMethod(getStopSessionGenerationMethod())
              .addMethod(getStreamSessionPartsMethod())
              .addMethod(getStreamSessionPartsBatchMethod())
              .addMethod(getPostChannelMessageMethod())
              .addMethod(getGetChannelMessageMethod())
              .addMethod(getListChannelMessagesMethod())
              .addMethod(getStreamChannelEventsMethod())
              .addMethod(getCreateWorkflowRunMethod())
              .addMethod(getGetWorkflowRunMethod())
              .addMethod(getListWorkflowRunsMethod())
              .addMethod(getResumeWorkflowRunMethod())
              .addMethod(getCancelWorkflowRunMethod())
              .addMethod(getStreamWorkflowEventsMethod())
              .addMethod(getCreateNamespaceMethod())
              .addMethod(getGetNamespaceMethod())
              .addMethod(getDeleteNamespaceMethod())
              .addMethod(getListNamespacesMethod())
              .addMethod(getCreateResourceMethod())
              .addMethod(getGetResourceMethod())
              .addMethod(getListResourcesMethod())
              .addMethod(getDeleteResourceMethod())
              .build();
        }
      }
    }
    return result;
  }
}
