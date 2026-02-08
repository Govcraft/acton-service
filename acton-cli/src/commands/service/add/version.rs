use anyhow::Result;
use colored::Colorize;

pub async fn execute(version: String, from: Option<String>, dry_run: bool) -> Result<()> {
    let version_upper = version.to_uppercase();
    let version_lower = version.to_lowercase();

    if dry_run {
        show_dry_run(&version_upper, &from);
        return Ok(());
    }

    println!(
        "{}",
        format!("Adding API version {}...", version_upper).bold()
    );
    println!();

    println!(
        "{}",
        "acton-service uses type-safe API versioning with VersionedApiBuilder.".bold()
    );
    println!();

    if let Some(ref source_version) = from {
        println!(
            "To add {} based on {}:",
            version_upper.cyan(),
            source_version.to_uppercase().cyan()
        );
    } else {
        println!("To add {} to your service:", version_upper.cyan());
    }

    println!();
    println!("{}", "1. Update your main.rs:".green().bold());
    println!();
    println!("   let routes = VersionedApiBuilder::new()");
    println!("       .with_base_path(\"/api\")");

    if let Some(source) = &from {
        let source_upper = source.to_uppercase();
        println!("       // Existing {} routes", source_upper);
        println!(
            "       .add_version(ApiVersion::{}, |routes| {{",
            source_upper
        );
        println!(
            "           routes.route(\"/users\", get(list_users_{}))  // existing",
            source.to_lowercase()
        );
        println!("       }})");
    }

    println!("       // New {} routes", version_upper);
    println!(
        "       .add_version(ApiVersion::{}, |routes| {{",
        version_upper
    );
    println!("           routes");
    println!(
        "               .route(\"/users\", get(list_users_{}))  // your handlers",
        version_lower
    );
    println!(
        "               .route(\"/users/{{id}}\", get(get_user_{}))",
        version_lower
    );
    println!("       }})");
    println!("       .build_routes();");
    println!();

    println!("{}", "2. Create handler functions:".green().bold());
    println!();
    println!(
        "   async fn list_users_{}() -> Json<Vec<User{}>> {{",
        version_lower, version_upper
    );
    println!("       // Implement your logic");
    println!("       Json(vec![])");
    println!("   }}");
    println!();
    println!(
        "   async fn get_user_{}(Path(id): Path<String>) -> Json<User{}> {{",
        version_lower, version_upper
    );
    println!("       // Implement your logic");
    println!("   }}");
    println!();

    if from.is_some() {
        println!(
            "{}",
            "3. Define version-specific types (if schema changed):"
                .green()
                .bold()
        );
        println!();
        println!("   #[derive(Serialize)]");
        println!("   struct User{} {{", version_upper);
        println!("       // Your fields - can differ from other versions");
        println!("   }}");
        println!();
    }

    println!("{}", "4. Optional: Deprecate old versions:".green().bold());
    println!();
    println!("   .add_version_deprecated(");
    println!("       ApiVersion::V1,");
    println!("       |routes| {{ /* routes */ }},");
    println!(
        "       DeprecationInfo::new(ApiVersion::V1, ApiVersion::{})",
        version_upper
    );
    println!("           .with_sunset_date(\"2026-12-31T23:59:59Z\")");
    println!("           .with_message(\"Migrate to {}\")", version_upper);
    println!("   )");
    println!();

    println!("{}", "Example usage:".cyan().bold());
    println!("  GET /api/{}/users", version_lower);
    println!("  GET /api/{}/users/{{id}}", version_lower);
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/examples/users-api.rs for a complete example");

    Ok(())
}

fn show_dry_run(version: &str, from: &Option<String>) {
    println!("\n{}", "Dry run - would show:".bold());
    println!();
    println!("Version: {}", version.cyan());
    if let Some(source) = from {
        println!("Copy from: {}", source.to_uppercase().cyan());
    }
    println!();
    println!(
        "Instructions for adding version {} to your service",
        version
    );
}
