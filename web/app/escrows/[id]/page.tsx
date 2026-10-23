import { Suspense } from "react";
import { EscrowView } from "@/components/escrow/EscrowView";
import { Spinner } from "@/components/ui";

// The id is only known at request time, so the client view (which reads it with useParams)
// sits inside a Suspense boundary.
export default function Page() {
  return (
    <Suspense fallback={<Spinner />}>
      <EscrowView />
    </Suspense>
  );
}
