import type { Metadata } from "next";
import { SiteFrame } from "../components/site/SiteFrame";
import { Capabilities } from "../components/site/home/Capabilities";
import { ClaudeBand } from "../components/site/home/ClaudeBand";
import { CtaBand } from "../components/site/home/CtaBand";
import { DemoStage } from "../components/site/home/DemoStage";
import { Engines } from "../components/site/home/Engines";
import { Faq } from "../components/site/home/Faq";
import { Founder } from "../components/site/home/Founder";
import { Hero } from "../components/site/home/Hero";
import { Modes } from "../components/site/home/Modes";
import { PlanTeaser } from "../components/site/home/PlanTeaser";
import { ReleaseBento } from "../components/site/home/ReleaseBento";
import { Roadmap } from "../components/site/home/Roadmap";
import { StatsBand } from "../components/site/home/StatsBand";
import { TrustFlow } from "../components/site/home/TrustFlow";
import { VideoSection } from "../components/site/home/VideoSection";

export const metadata: Metadata = { alternates: { canonical: "/" } };

/** The home page (F-WB-01, §4.3): the promise, proof you can touch and
 * proof in numbers, what it works with and does, why you can trust it, then
 * the video, what's next, cost, doubts and the ask. Content lives in
 * lib/home.ts; every figure comes from code. */
export default function HomePage() {
  return (
    <SiteFrame current="/">
      <Hero />
      <DemoStage />
      <StatsBand />
      <Engines />
      <Modes />
      <ReleaseBento />
      <Capabilities />
      <TrustFlow />
      <ClaudeBand />
      <Founder />
      <VideoSection />
      <Roadmap />
      <PlanTeaser />
      <Faq />
      <CtaBand />
    </SiteFrame>
  );
}
