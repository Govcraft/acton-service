// Cedar policies for the cedar-grpc example.
//
// gRPC actions are the full method path: Action::"/package.Service/Method".
// The context carries the authenticated principal's roles and permissions.

// Any authenticated principal with the "user" role may call SayHello.
permit(
    principal,
    action == Action::"/hello.v1.HelloService/SayHello",
    resource
) when { context.roles.contains("user") };
