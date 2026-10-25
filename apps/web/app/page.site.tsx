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
import { StatsBand } from "../components/site/home/StatsBand";
import { TrustFlow } from "../components/site/home/TrustFlow";

export const metadata: Metadata = { alternates: { canonical: "/" } };

/** The home page (F-WB-01, D-1841): ownership first, then the product you
 * can touch, how your data moves, what it works with and does, who builds it,
 * what it costs, doubts and the ask. The release tiles and the roadmap live
 * on /changelog/, the video on /demo/. Content lives in lib/home.ts; every
 * figure comes from code. */
export default function HomePage() {
  return (
    <SiteFrame current="/">
      <Hero />
      <DemoStage />
      <TrustFlow />
      <Engines />
      <Modes />
      <Capabilities />
      <ClaudeBand />
      <StatsBand />
      <Founder />
      <Faq />
      <CtaBand />
    </SiteFrame>
  );
}
