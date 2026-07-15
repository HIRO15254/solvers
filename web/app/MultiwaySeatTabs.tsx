"use client";

import { type KeyboardEvent, useState } from "react";
import type { MultiwaySeat } from "./preflop-config";

interface Props {
  seats: MultiwaySeat[];
}

export default function MultiwaySeatTabs({ seats }: Props) {
  const [active, setActive] = useState(0);
  const selected = Math.min(active, Math.max(0, seats.length - 1));


  function select(index: number, focus = false) {
    setActive(index);
    document
      .getElementById(`multiway-seat-row-${index}`)
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    if (focus) document.getElementById(`multiway-seat-tab-${index}`)?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const current = Number(
      (event.target as HTMLButtonElement).dataset.seatIndex,
    );
    if (!Number.isInteger(current)) return;
    let next = current;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      next = (current + 1) % seats.length;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      next = (current - 1 + seats.length) % seats.length;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = seats.length - 1;
    } else {
      return;
    }
    event.preventDefault();
    select(next, true);
  }

  return (
    <div
      className="range-tabs seat-quick-nav"
      role="toolbar"
      aria-label="Seat quick navigation"
      aria-orientation="horizontal"
      onKeyDown={onKeyDown}
    >
      {seats.map((seat, index) => (
        <button
          id={`multiway-seat-tab-${index}`}
          key={seat.id}
          type="button"
          className="range-tab"
          aria-pressed={selected === index}
          aria-controls={`multiway-seat-row-${index}`}
          tabIndex={selected === index ? 0 : -1}
          data-seat-index={index}
          onClick={() => select(index)}
        >
          {seat.position}
        </button>
      ))}
    </div>
  );
}
