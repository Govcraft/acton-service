// Example Cedar Policies for Document Management API
//
// This file demonstrates common authorization patterns using Cedar.
// Copy this to: ~/.config/acton-service/cedar-authz-example/policies.cedar

// =============================================================================
// Admin Policies
// =============================================================================

// Admins can perform any action on any resource
permit(
    principal,
    action,
    resource
)
when {
    principal.roles.contains("admin")
};

// =============================================================================
// Document Listing Policies
// =============================================================================

// Any authenticated user can list documents
permit(
    principal,
    action == Action::"GET /api/v1/documents",
    resource
);

// =============================================================================
// Document Creation Policies
// =============================================================================

// Any authenticated user can create documents
permit(
    principal,
    action == Action::"POST /api/v1/documents",
    resource
);

// =============================================================================
// Document Access Policies (Ownership-based)
// =============================================================================

// Users can access their own documents (read, update, delete)
permit(
    principal,
    action in [
        Action::"GET /api/v1/documents/{user_id}/{doc_id}",
        Action::"PUT /api/v1/documents/{user_id}/{doc_id}",
        Action::"DELETE /api/v1/documents/{user_id}/{doc_id}"
    ],
    resource
)
when {
    // The principal's sub claim must match the resource owner
    principal.sub == resource.owner_id
};

// =============================================================================
// Admin-Only Endpoints
// =============================================================================

// Only admins can list all users
forbid(
    principal,
    action == Action::"GET /api/v1/admin/users",
    resource
)
unless {
    principal.roles.contains("admin")
};

// =============================================================================
// Time-based Access Policies (Example)
// =============================================================================

// Block access to sensitive endpoints outside business hours (optional example)
// Uncomment to enable:
//
// forbid(
//     principal,
//     action == Action::"DELETE /api/v1/documents/:user_id/:doc_id",
//     resource
// )
// when {
//     context.hour < 9 || context.hour >= 17
// }
// unless {
//     principal.roles.contains("admin")
// };

// =============================================================================
// IP-based Access Policies (Example)
// =============================================================================

// Restrict admin endpoints to internal network (optional example)
// Uncomment to enable:
//
// forbid(
//     principal,
//     action in [
//         Action::"GET /api/v1/admin/users"
//     ],
//     resource
// )
// when {
//     !context.ip_address.isInRange(ip("10.0.0.0/8"))
// }
// unless {
//     principal.roles.contains("super_admin")
// };
