CREATE TABLE "org_sso" (
	"org_id" text PRIMARY KEY NOT NULL,
	"provider" text NOT NULL,
	"issuer" text,
	"client_id" text,
	"client_secret_enc" text,
	"organization" text,
	"enforce" boolean DEFAULT false NOT NULL,
	"updated_at" timestamp with time zone NOT NULL
);
--> statement-breakpoint
CREATE TABLE "org_sso_domains" (
	"org_id" text NOT NULL,
	"domain" text NOT NULL,
	"verify_token" text NOT NULL,
	"verified_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "org_sso_domains_org_id_domain_pk" PRIMARY KEY("org_id","domain")
);
--> statement-breakpoint
ALTER TABLE "org_sso" ADD CONSTRAINT "org_sso_org_id_orgs_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."orgs"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "org_sso_domains" ADD CONSTRAINT "org_sso_domains_org_id_orgs_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."orgs"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "org_sso_domains_verified" ON "org_sso_domains" USING btree ("domain") WHERE "org_sso_domains"."verified_at" is not null;