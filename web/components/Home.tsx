"use client";

import Link from "next/link";
import { useState } from "react";
import type { NewUser } from "@/lib/schemas";
import { useSession } from "@/lib/session";
import { TokenReveal } from "./Account";
import { Dashboard } from "./dashboard/Dashboard";
import { Landing } from "./landing/Landing";
import { Card } from "./ui";

export function Home() {
  const session = useSession();
  const [created, setCreated] = useState<NewUser | null>(null);

  if (session === undefined) return null;

  if (created) {
    return (
      <div className="mx-auto w-full max-w-xl">
        <TokenReveal user={created} onDone={() => setCreated(null)} />
      </div>
    );
  }

  if (!session) return <Landing onCreated={setCreated} />;

  if (session.kind === "operator") {
    return (
      <Card title="Signed in as operator">
        <p className="text-sm">
          Operators review the queue and can cancel escrows.{" "}
          <Link href="/operator" className="font-semibold text-amber-600 underline">
            Open the operator queue
          </Link>
          .
        </p>
      </Card>
    );
  }

  return <Dashboard session={session} />;
}
