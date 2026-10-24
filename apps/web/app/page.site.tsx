import type { Metadata } from "next";
// The product scenes' styles load with the pages that show them (D-1880).
import "../components/site/scenes/scenes.css";
import { SiteFrame } from "../components/site/SiteFrame";
import { Capabilities } from "../components/site/home/Capabilities";
import { CtaBand } from "../components/site/home/CtaBand";
import { DemoStage } from "../components/site/home/DemoStage";
import { Engines } from "../components/site/home/Engines";
import { Faq } from "../components/site/home/Faq";
import { Features } from "../components/site/home/Features";
import { Hero } from "../components/site/home/Hero";
import { PlanTeaser } from "../components/site/home/PlanTeaser";
import { TrustFlow } from "../components/site/home/TrustFlow";

export const metadata: Metadata = { alternates: { canonical: "/" } };

/** The home page (F-WB-01, D-1841, D-1881): ownership first with the product
 * in the first viewport, then the product you can touch, what it's like (as
 * scenes), how your data moves, what it works with, the rest of what it does,
 * what it costs, four doubts and the ask. Built with Claude and the figures
 * live on /about/, the release tiles and the roadmap on /changelog/, the
 * video on /demo/. Content lives in lib/home.ts; every figure comes from code. */
export default function HomePage() {
  return (
    <SiteFrame current="/">
      <Hero />
      <DemoStage />
      <Features />
      <TrustFlow />
      <Engines />
      <Capabilities />
      <PlanTeaser />
      <Faq />
      <CtaBand />
    </SiteFrame>
  );
}
