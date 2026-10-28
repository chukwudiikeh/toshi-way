import { notFound } from "next/navigation";
import { getOrder, type OrderStatus } from "@/lib/orders";
import { Refresh } from "./Refresh";

// Always read the latest order, never a cached copy.
export const dynamic = "force-dynamic";

const words: Record<OrderStatus, { title: string; body: string }> = {
  awaiting_payment: { title: "Waiting for payment", body: "Finish paying on toshi-way. This page updates by itself." },
  paid_into_escrow: { title: "Paid and locked", body: "Your money is locked. The shop will deliver, then you confirm to release it." },
  paid: { title: "Complete", body: "Delivery confirmed and the shop has been paid." },
  cancelled: { title: "Cancelled", body: "The order did not go through. Nothing was taken, or it was refunded." },
};

export default async function OrderPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const order = await getOrder(id);
  if (!order) notFound();
  const w = words[order.status];
  return (
    <main>
      <h1>Order {order.id}</h1>
      <p>
        {order.item} — {order.amount_sats.toLocaleString()} sats
      </p>
      <h2>{w.title}</h2>
      <p>{w.body}</p>
      {/* Arriving on this page proves nothing. The status above only changes when toshi-way's webhook says so. */}
      {(order.status === "awaiting_payment" || order.status === "paid_into_escrow") && <Refresh />}
    </main>
  );
}
