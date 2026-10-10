ALTER TABLE "voice_sessions" ADD COLUMN "provider_session_id" text;--> statement-breakpoint
ALTER TABLE "voice_sessions" ADD COLUMN "last_act_at" timestamp with time zone;--> statement-breakpoint
CREATE INDEX "voice_open" ON "voice_sessions" USING btree ("expires_at") WHERE "voice_sessions"."settled_at" is null;