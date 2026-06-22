package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class SessionServiceGrpc {

  private SessionServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.SessionService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.CreateSessionRequest,
      talon.v1.Sessions.SessionResponse> getCreateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Create",
      requestType = talon.v1.Sessions.CreateSessionRequest.class,
      responseType = talon.v1.Sessions.SessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.CreateSessionRequest,
      talon.v1.Sessions.SessionResponse> getCreateMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.CreateSessionRequest, talon.v1.Sessions.SessionResponse> getCreateMethod;
    if ((getCreateMethod = SessionServiceGrpc.getCreateMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getCreateMethod = SessionServiceGrpc.getCreateMethod) == null) {
          SessionServiceGrpc.getCreateMethod = getCreateMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.CreateSessionRequest, talon.v1.Sessions.SessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Create"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.CreateSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.SessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("Create"))
              .build();
        }
      }
    }
    return getCreateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.GetSessionRequest,
      talon.v1.Sessions.SessionResponse> getGetMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Get",
      requestType = talon.v1.Sessions.GetSessionRequest.class,
      responseType = talon.v1.Sessions.SessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.GetSessionRequest,
      talon.v1.Sessions.SessionResponse> getGetMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.GetSessionRequest, talon.v1.Sessions.SessionResponse> getGetMethod;
    if ((getGetMethod = SessionServiceGrpc.getGetMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getGetMethod = SessionServiceGrpc.getGetMethod) == null) {
          SessionServiceGrpc.getGetMethod = getGetMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.GetSessionRequest, talon.v1.Sessions.SessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Get"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.GetSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.SessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("Get"))
              .build();
        }
      }
    }
    return getGetMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionsRequest,
      talon.v1.Sessions.ListSessionsResponse> getListMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "List",
      requestType = talon.v1.Sessions.ListSessionsRequest.class,
      responseType = talon.v1.Sessions.ListSessionsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionsRequest,
      talon.v1.Sessions.ListSessionsResponse> getListMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionsRequest, talon.v1.Sessions.ListSessionsResponse> getListMethod;
    if ((getListMethod = SessionServiceGrpc.getListMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getListMethod = SessionServiceGrpc.getListMethod) == null) {
          SessionServiceGrpc.getListMethod = getListMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.ListSessionsRequest, talon.v1.Sessions.ListSessionsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "List"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ListSessionsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ListSessionsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("List"))
              .build();
        }
      }
    }
    return getListMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionMessagesRequest,
      talon.v1.Sessions.ListSessionMessagesResponse> getListMessagesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListMessages",
      requestType = talon.v1.Sessions.ListSessionMessagesRequest.class,
      responseType = talon.v1.Sessions.ListSessionMessagesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionMessagesRequest,
      talon.v1.Sessions.ListSessionMessagesResponse> getListMessagesMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.ListSessionMessagesRequest, talon.v1.Sessions.ListSessionMessagesResponse> getListMessagesMethod;
    if ((getListMessagesMethod = SessionServiceGrpc.getListMessagesMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getListMessagesMethod = SessionServiceGrpc.getListMessagesMethod) == null) {
          SessionServiceGrpc.getListMessagesMethod = getListMessagesMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.ListSessionMessagesRequest, talon.v1.Sessions.ListSessionMessagesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListMessages"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ListSessionMessagesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ListSessionMessagesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("ListMessages"))
              .build();
        }
      }
    }
    return getListMessagesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.DeleteSessionRequest,
      talon.v1.Sessions.DeleteSessionResponse> getDeleteMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Delete",
      requestType = talon.v1.Sessions.DeleteSessionRequest.class,
      responseType = talon.v1.Sessions.DeleteSessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.DeleteSessionRequest,
      talon.v1.Sessions.DeleteSessionResponse> getDeleteMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.DeleteSessionRequest, talon.v1.Sessions.DeleteSessionResponse> getDeleteMethod;
    if ((getDeleteMethod = SessionServiceGrpc.getDeleteMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getDeleteMethod = SessionServiceGrpc.getDeleteMethod) == null) {
          SessionServiceGrpc.getDeleteMethod = getDeleteMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.DeleteSessionRequest, talon.v1.Sessions.DeleteSessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Delete"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.DeleteSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.DeleteSessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("Delete"))
              .build();
        }
      }
    }
    return getDeleteMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.ClearSessionRequest,
      talon.v1.Sessions.ClearSessionResponse> getClearMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Clear",
      requestType = talon.v1.Sessions.ClearSessionRequest.class,
      responseType = talon.v1.Sessions.ClearSessionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.ClearSessionRequest,
      talon.v1.Sessions.ClearSessionResponse> getClearMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.ClearSessionRequest, talon.v1.Sessions.ClearSessionResponse> getClearMethod;
    if ((getClearMethod = SessionServiceGrpc.getClearMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getClearMethod = SessionServiceGrpc.getClearMethod) == null) {
          SessionServiceGrpc.getClearMethod = getClearMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.ClearSessionRequest, talon.v1.Sessions.ClearSessionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Clear"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ClearSessionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.ClearSessionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("Clear"))
              .build();
        }
      }
    }
    return getClearMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.SendMessageRequest,
      talon.v1.Sessions.SendMessageResponse> getSendMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "SendMessage",
      requestType = talon.v1.Sessions.SendMessageRequest.class,
      responseType = talon.v1.Sessions.SendMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.SendMessageRequest,
      talon.v1.Sessions.SendMessageResponse> getSendMessageMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.SendMessageRequest, talon.v1.Sessions.SendMessageResponse> getSendMessageMethod;
    if ((getSendMessageMethod = SessionServiceGrpc.getSendMessageMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getSendMessageMethod = SessionServiceGrpc.getSendMessageMethod) == null) {
          SessionServiceGrpc.getSendMessageMethod = getSendMessageMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.SendMessageRequest, talon.v1.Sessions.SendMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "SendMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.SendMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.SendMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("SendMessage"))
              .build();
        }
      }
    }
    return getSendMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.AppendSessionMessageRequest,
      talon.v1.Sessions.AppendSessionMessageResponse> getAppendMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "AppendMessage",
      requestType = talon.v1.Sessions.AppendSessionMessageRequest.class,
      responseType = talon.v1.Sessions.AppendSessionMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.AppendSessionMessageRequest,
      talon.v1.Sessions.AppendSessionMessageResponse> getAppendMessageMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.AppendSessionMessageRequest, talon.v1.Sessions.AppendSessionMessageResponse> getAppendMessageMethod;
    if ((getAppendMessageMethod = SessionServiceGrpc.getAppendMessageMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getAppendMessageMethod = SessionServiceGrpc.getAppendMessageMethod) == null) {
          SessionServiceGrpc.getAppendMessageMethod = getAppendMessageMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.AppendSessionMessageRequest, talon.v1.Sessions.AppendSessionMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "AppendMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.AppendSessionMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.AppendSessionMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("AppendMessage"))
              .build();
        }
      }
    }
    return getAppendMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.AnswerSessionPermissionRequest,
      talon.v1.Sessions.AnswerSessionPermissionResponse> getAnswerPermissionMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "AnswerPermission",
      requestType = talon.v1.Sessions.AnswerSessionPermissionRequest.class,
      responseType = talon.v1.Sessions.AnswerSessionPermissionResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.AnswerSessionPermissionRequest,
      talon.v1.Sessions.AnswerSessionPermissionResponse> getAnswerPermissionMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.AnswerSessionPermissionRequest, talon.v1.Sessions.AnswerSessionPermissionResponse> getAnswerPermissionMethod;
    if ((getAnswerPermissionMethod = SessionServiceGrpc.getAnswerPermissionMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getAnswerPermissionMethod = SessionServiceGrpc.getAnswerPermissionMethod) == null) {
          SessionServiceGrpc.getAnswerPermissionMethod = getAnswerPermissionMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.AnswerSessionPermissionRequest, talon.v1.Sessions.AnswerSessionPermissionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "AnswerPermission"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.AnswerSessionPermissionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.AnswerSessionPermissionResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("AnswerPermission"))
              .build();
        }
      }
    }
    return getAnswerPermissionMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.StopSessionGenerationRequest,
      talon.v1.Sessions.StopSessionGenerationResponse> getStopGenerationMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StopGeneration",
      requestType = talon.v1.Sessions.StopSessionGenerationRequest.class,
      responseType = talon.v1.Sessions.StopSessionGenerationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.StopSessionGenerationRequest,
      talon.v1.Sessions.StopSessionGenerationResponse> getStopGenerationMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.StopSessionGenerationRequest, talon.v1.Sessions.StopSessionGenerationResponse> getStopGenerationMethod;
    if ((getStopGenerationMethod = SessionServiceGrpc.getStopGenerationMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getStopGenerationMethod = SessionServiceGrpc.getStopGenerationMethod) == null) {
          SessionServiceGrpc.getStopGenerationMethod = getStopGenerationMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.StopSessionGenerationRequest, talon.v1.Sessions.StopSessionGenerationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StopGeneration"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.StopSessionGenerationRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.StopSessionGenerationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("StopGeneration"))
              .build();
        }
      }
    }
    return getStopGenerationMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamPartsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamParts",
      requestType = talon.v1.Sessions.StreamSessionPartsRequest.class,
      responseType = talon.events.Events.SessionMessagePartEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamPartsMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsRequest, talon.events.Events.SessionMessagePartEvent> getStreamPartsMethod;
    if ((getStreamPartsMethod = SessionServiceGrpc.getStreamPartsMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getStreamPartsMethod = SessionServiceGrpc.getStreamPartsMethod) == null) {
          SessionServiceGrpc.getStreamPartsMethod = getStreamPartsMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.StreamSessionPartsRequest, talon.events.Events.SessionMessagePartEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamParts"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.StreamSessionPartsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.SessionMessagePartEvent.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("StreamParts"))
              .build();
        }
      }
    }
    return getStreamPartsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsBatchRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamPartsBatchMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamPartsBatch",
      requestType = talon.v1.Sessions.StreamSessionPartsBatchRequest.class,
      responseType = talon.events.Events.SessionMessagePartEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsBatchRequest,
      talon.events.Events.SessionMessagePartEvent> getStreamPartsBatchMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.StreamSessionPartsBatchRequest, talon.events.Events.SessionMessagePartEvent> getStreamPartsBatchMethod;
    if ((getStreamPartsBatchMethod = SessionServiceGrpc.getStreamPartsBatchMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getStreamPartsBatchMethod = SessionServiceGrpc.getStreamPartsBatchMethod) == null) {
          SessionServiceGrpc.getStreamPartsBatchMethod = getStreamPartsBatchMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.StreamSessionPartsBatchRequest, talon.events.Events.SessionMessagePartEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamPartsBatch"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.StreamSessionPartsBatchRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.SessionMessagePartEvent.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("StreamPartsBatch"))
              .build();
        }
      }
    }
    return getStreamPartsBatchMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Sessions.SubmitSessionTurnRequest,
      talon.events.Events.SessionMessagePartEvent> getSubmitTurnMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "SubmitTurn",
      requestType = talon.v1.Sessions.SubmitSessionTurnRequest.class,
      responseType = talon.events.Events.SessionMessagePartEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.v1.Sessions.SubmitSessionTurnRequest,
      talon.events.Events.SessionMessagePartEvent> getSubmitTurnMethod() {
    io.grpc.MethodDescriptor<talon.v1.Sessions.SubmitSessionTurnRequest, talon.events.Events.SessionMessagePartEvent> getSubmitTurnMethod;
    if ((getSubmitTurnMethod = SessionServiceGrpc.getSubmitTurnMethod) == null) {
      synchronized (SessionServiceGrpc.class) {
        if ((getSubmitTurnMethod = SessionServiceGrpc.getSubmitTurnMethod) == null) {
          SessionServiceGrpc.getSubmitTurnMethod = getSubmitTurnMethod =
              io.grpc.MethodDescriptor.<talon.v1.Sessions.SubmitSessionTurnRequest, talon.events.Events.SessionMessagePartEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "SubmitTurn"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Sessions.SubmitSessionTurnRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.SessionMessagePartEvent.getDefaultInstance()))
              .setSchemaDescriptor(new SessionServiceMethodDescriptorSupplier("SubmitTurn"))
              .build();
        }
      }
    }
    return getSubmitTurnMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static SessionServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<SessionServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<SessionServiceStub>() {
        @java.lang.Override
        public SessionServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new SessionServiceStub(channel, callOptions);
        }
      };
    return SessionServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static SessionServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<SessionServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<SessionServiceBlockingV2Stub>() {
        @java.lang.Override
        public SessionServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new SessionServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return SessionServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static SessionServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<SessionServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<SessionServiceBlockingStub>() {
        @java.lang.Override
        public SessionServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new SessionServiceBlockingStub(channel, callOptions);
        }
      };
    return SessionServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static SessionServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<SessionServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<SessionServiceFutureStub>() {
        @java.lang.Override
        public SessionServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new SessionServiceFutureStub(channel, callOptions);
        }
      };
    return SessionServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void create(talon.v1.Sessions.CreateSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateMethod(), responseObserver);
    }

    /**
     */
    default void get(talon.v1.Sessions.GetSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMethod(), responseObserver);
    }

    /**
     */
    default void list(talon.v1.Sessions.ListSessionsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMethod(), responseObserver);
    }

    /**
     */
    default void listMessages(talon.v1.Sessions.ListSessionMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionMessagesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMessagesMethod(), responseObserver);
    }

    /**
     */
    default void delete(talon.v1.Sessions.DeleteSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.DeleteSessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteMethod(), responseObserver);
    }

    /**
     */
    default void clear(talon.v1.Sessions.ClearSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ClearSessionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getClearMethod(), responseObserver);
    }

    /**
     */
    default void sendMessage(talon.v1.Sessions.SendMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SendMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getSendMessageMethod(), responseObserver);
    }

    /**
     */
    default void appendMessage(talon.v1.Sessions.AppendSessionMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.AppendSessionMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getAppendMessageMethod(), responseObserver);
    }

    /**
     */
    default void answerPermission(talon.v1.Sessions.AnswerSessionPermissionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.AnswerSessionPermissionResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getAnswerPermissionMethod(), responseObserver);
    }

    /**
     */
    default void stopGeneration(talon.v1.Sessions.StopSessionGenerationRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.StopSessionGenerationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStopGenerationMethod(), responseObserver);
    }

    /**
     */
    default void streamParts(talon.v1.Sessions.StreamSessionPartsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamPartsMethod(), responseObserver);
    }

    /**
     */
    default void streamPartsBatch(talon.v1.Sessions.StreamSessionPartsBatchRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamPartsBatchMethod(), responseObserver);
    }

    /**
     */
    default void submitTurn(talon.v1.Sessions.SubmitSessionTurnRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getSubmitTurnMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service SessionService.
   */
  public static abstract class SessionServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return SessionServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service SessionService.
   */
  public static final class SessionServiceStub
      extends io.grpc.stub.AbstractAsyncStub<SessionServiceStub> {
    private SessionServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected SessionServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new SessionServiceStub(channel, callOptions);
    }

    /**
     */
    public void create(talon.v1.Sessions.CreateSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void get(talon.v1.Sessions.GetSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void list(talon.v1.Sessions.ListSessionsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listMessages(talon.v1.Sessions.ListSessionMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionMessagesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMessagesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void delete(talon.v1.Sessions.DeleteSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.DeleteSessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void clear(talon.v1.Sessions.ClearSessionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.ClearSessionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getClearMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void sendMessage(talon.v1.Sessions.SendMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.SendMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getSendMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void appendMessage(talon.v1.Sessions.AppendSessionMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.AppendSessionMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getAppendMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void answerPermission(talon.v1.Sessions.AnswerSessionPermissionRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.AnswerSessionPermissionResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getAnswerPermissionMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void stopGeneration(talon.v1.Sessions.StopSessionGenerationRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Sessions.StopSessionGenerationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getStopGenerationMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamParts(talon.v1.Sessions.StreamSessionPartsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamPartsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamPartsBatch(talon.v1.Sessions.StreamSessionPartsBatchRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamPartsBatchMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void submitTurn(talon.v1.Sessions.SubmitSessionTurnRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getSubmitTurnMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service SessionService.
   */
  public static final class SessionServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<SessionServiceBlockingV2Stub> {
    private SessionServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected SessionServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new SessionServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Sessions.SessionResponse create(talon.v1.Sessions.CreateSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.SessionResponse get(talon.v1.Sessions.GetSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ListSessionsResponse list(talon.v1.Sessions.ListSessionsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ListSessionMessagesResponse listMessages(talon.v1.Sessions.ListSessionMessagesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.DeleteSessionResponse delete(talon.v1.Sessions.DeleteSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ClearSessionResponse clear(talon.v1.Sessions.ClearSessionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getClearMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.SendMessageResponse sendMessage(talon.v1.Sessions.SendMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getSendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.AppendSessionMessageResponse appendMessage(talon.v1.Sessions.AppendSessionMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getAppendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.AnswerSessionPermissionResponse answerPermission(talon.v1.Sessions.AnswerSessionPermissionRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getAnswerPermissionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.StopSessionGenerationResponse stopGeneration(talon.v1.Sessions.StopSessionGenerationRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getStopGenerationMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.SessionMessagePartEvent>
        streamParts(talon.v1.Sessions.StreamSessionPartsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamPartsMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.SessionMessagePartEvent>
        streamPartsBatch(talon.v1.Sessions.StreamSessionPartsBatchRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamPartsBatchMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.SessionMessagePartEvent>
        submitTurn(talon.v1.Sessions.SubmitSessionTurnRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getSubmitTurnMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service SessionService.
   */
  public static final class SessionServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<SessionServiceBlockingStub> {
    private SessionServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected SessionServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new SessionServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Sessions.SessionResponse create(talon.v1.Sessions.CreateSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.SessionResponse get(talon.v1.Sessions.GetSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ListSessionsResponse list(talon.v1.Sessions.ListSessionsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ListSessionMessagesResponse listMessages(talon.v1.Sessions.ListSessionMessagesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.DeleteSessionResponse delete(talon.v1.Sessions.DeleteSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.ClearSessionResponse clear(talon.v1.Sessions.ClearSessionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getClearMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.SendMessageResponse sendMessage(talon.v1.Sessions.SendMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getSendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.AppendSessionMessageResponse appendMessage(talon.v1.Sessions.AppendSessionMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getAppendMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.AnswerSessionPermissionResponse answerPermission(talon.v1.Sessions.AnswerSessionPermissionRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getAnswerPermissionMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Sessions.StopSessionGenerationResponse stopGeneration(talon.v1.Sessions.StopSessionGenerationRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getStopGenerationMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.SessionMessagePartEvent> streamParts(
        talon.v1.Sessions.StreamSessionPartsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamPartsMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.SessionMessagePartEvent> streamPartsBatch(
        talon.v1.Sessions.StreamSessionPartsBatchRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamPartsBatchMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.SessionMessagePartEvent> submitTurn(
        talon.v1.Sessions.SubmitSessionTurnRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getSubmitTurnMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service SessionService.
   */
  public static final class SessionServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<SessionServiceFutureStub> {
    private SessionServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected SessionServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new SessionServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.SessionResponse> create(
        talon.v1.Sessions.CreateSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.SessionResponse> get(
        talon.v1.Sessions.GetSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.ListSessionsResponse> list(
        talon.v1.Sessions.ListSessionsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.ListSessionMessagesResponse> listMessages(
        talon.v1.Sessions.ListSessionMessagesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMessagesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.DeleteSessionResponse> delete(
        talon.v1.Sessions.DeleteSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.ClearSessionResponse> clear(
        talon.v1.Sessions.ClearSessionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getClearMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.SendMessageResponse> sendMessage(
        talon.v1.Sessions.SendMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getSendMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.AppendSessionMessageResponse> appendMessage(
        talon.v1.Sessions.AppendSessionMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getAppendMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.AnswerSessionPermissionResponse> answerPermission(
        talon.v1.Sessions.AnswerSessionPermissionRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getAnswerPermissionMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Sessions.StopSessionGenerationResponse> stopGeneration(
        talon.v1.Sessions.StopSessionGenerationRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getStopGenerationMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE = 0;
  private static final int METHODID_GET = 1;
  private static final int METHODID_LIST = 2;
  private static final int METHODID_LIST_MESSAGES = 3;
  private static final int METHODID_DELETE = 4;
  private static final int METHODID_CLEAR = 5;
  private static final int METHODID_SEND_MESSAGE = 6;
  private static final int METHODID_APPEND_MESSAGE = 7;
  private static final int METHODID_ANSWER_PERMISSION = 8;
  private static final int METHODID_STOP_GENERATION = 9;
  private static final int METHODID_STREAM_PARTS = 10;
  private static final int METHODID_STREAM_PARTS_BATCH = 11;
  private static final int METHODID_SUBMIT_TURN = 12;

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
        case METHODID_CREATE:
          serviceImpl.create((talon.v1.Sessions.CreateSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse>) responseObserver);
          break;
        case METHODID_GET:
          serviceImpl.get((talon.v1.Sessions.GetSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.SessionResponse>) responseObserver);
          break;
        case METHODID_LIST:
          serviceImpl.list((talon.v1.Sessions.ListSessionsRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionsResponse>) responseObserver);
          break;
        case METHODID_LIST_MESSAGES:
          serviceImpl.listMessages((talon.v1.Sessions.ListSessionMessagesRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.ListSessionMessagesResponse>) responseObserver);
          break;
        case METHODID_DELETE:
          serviceImpl.delete((talon.v1.Sessions.DeleteSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.DeleteSessionResponse>) responseObserver);
          break;
        case METHODID_CLEAR:
          serviceImpl.clear((talon.v1.Sessions.ClearSessionRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.ClearSessionResponse>) responseObserver);
          break;
        case METHODID_SEND_MESSAGE:
          serviceImpl.sendMessage((talon.v1.Sessions.SendMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.SendMessageResponse>) responseObserver);
          break;
        case METHODID_APPEND_MESSAGE:
          serviceImpl.appendMessage((talon.v1.Sessions.AppendSessionMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.AppendSessionMessageResponse>) responseObserver);
          break;
        case METHODID_ANSWER_PERMISSION:
          serviceImpl.answerPermission((talon.v1.Sessions.AnswerSessionPermissionRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.AnswerSessionPermissionResponse>) responseObserver);
          break;
        case METHODID_STOP_GENERATION:
          serviceImpl.stopGeneration((talon.v1.Sessions.StopSessionGenerationRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Sessions.StopSessionGenerationResponse>) responseObserver);
          break;
        case METHODID_STREAM_PARTS:
          serviceImpl.streamParts((talon.v1.Sessions.StreamSessionPartsRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent>) responseObserver);
          break;
        case METHODID_STREAM_PARTS_BATCH:
          serviceImpl.streamPartsBatch((talon.v1.Sessions.StreamSessionPartsBatchRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent>) responseObserver);
          break;
        case METHODID_SUBMIT_TURN:
          serviceImpl.submitTurn((talon.v1.Sessions.SubmitSessionTurnRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.SessionMessagePartEvent>) responseObserver);
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
          getCreateMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.CreateSessionRequest,
              talon.v1.Sessions.SessionResponse>(
                service, METHODID_CREATE)))
        .addMethod(
          getGetMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.GetSessionRequest,
              talon.v1.Sessions.SessionResponse>(
                service, METHODID_GET)))
        .addMethod(
          getListMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.ListSessionsRequest,
              talon.v1.Sessions.ListSessionsResponse>(
                service, METHODID_LIST)))
        .addMethod(
          getListMessagesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.ListSessionMessagesRequest,
              talon.v1.Sessions.ListSessionMessagesResponse>(
                service, METHODID_LIST_MESSAGES)))
        .addMethod(
          getDeleteMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.DeleteSessionRequest,
              talon.v1.Sessions.DeleteSessionResponse>(
                service, METHODID_DELETE)))
        .addMethod(
          getClearMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.ClearSessionRequest,
              talon.v1.Sessions.ClearSessionResponse>(
                service, METHODID_CLEAR)))
        .addMethod(
          getSendMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.SendMessageRequest,
              talon.v1.Sessions.SendMessageResponse>(
                service, METHODID_SEND_MESSAGE)))
        .addMethod(
          getAppendMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.AppendSessionMessageRequest,
              talon.v1.Sessions.AppendSessionMessageResponse>(
                service, METHODID_APPEND_MESSAGE)))
        .addMethod(
          getAnswerPermissionMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.AnswerSessionPermissionRequest,
              talon.v1.Sessions.AnswerSessionPermissionResponse>(
                service, METHODID_ANSWER_PERMISSION)))
        .addMethod(
          getStopGenerationMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Sessions.StopSessionGenerationRequest,
              talon.v1.Sessions.StopSessionGenerationResponse>(
                service, METHODID_STOP_GENERATION)))
        .addMethod(
          getStreamPartsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.v1.Sessions.StreamSessionPartsRequest,
              talon.events.Events.SessionMessagePartEvent>(
                service, METHODID_STREAM_PARTS)))
        .addMethod(
          getStreamPartsBatchMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.v1.Sessions.StreamSessionPartsBatchRequest,
              talon.events.Events.SessionMessagePartEvent>(
                service, METHODID_STREAM_PARTS_BATCH)))
        .addMethod(
          getSubmitTurnMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.v1.Sessions.SubmitSessionTurnRequest,
              talon.events.Events.SessionMessagePartEvent>(
                service, METHODID_SUBMIT_TURN)))
        .build();
  }

  private static abstract class SessionServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    SessionServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Sessions.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("SessionService");
    }
  }

  private static final class SessionServiceFileDescriptorSupplier
      extends SessionServiceBaseDescriptorSupplier {
    SessionServiceFileDescriptorSupplier() {}
  }

  private static final class SessionServiceMethodDescriptorSupplier
      extends SessionServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    SessionServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (SessionServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new SessionServiceFileDescriptorSupplier())
              .addMethod(getCreateMethod())
              .addMethod(getGetMethod())
              .addMethod(getListMethod())
              .addMethod(getListMessagesMethod())
              .addMethod(getDeleteMethod())
              .addMethod(getClearMethod())
              .addMethod(getSendMessageMethod())
              .addMethod(getAppendMessageMethod())
              .addMethod(getAnswerPermissionMethod())
              .addMethod(getStopGenerationMethod())
              .addMethod(getStreamPartsMethod())
              .addMethod(getStreamPartsBatchMethod())
              .addMethod(getSubmitTurnMethod())
              .build();
        }
      }
    }
    return result;
  }
}
