import {
  CircleCheckIcon,
  CircleDashedIcon,
  CircleHelpIcon,
  CircleMinusIcon,
  CircleXIcon,
  TriangleAlertIcon,
  type LucideIcon,
} from "lucide-react"

import type { Status } from "@/lib/rulekit"

export const STATUS: Record<
  Status,
  {
    label: string
    verdict: string
    icon: LucideIcon
    tone: string
    rail: string
  }
> = {
  passed: {
    label: "Passed",
    verdict: "Pass",
    icon: CircleCheckIcon,
    tone: "text-pass",
    rail: "border-l-pass",
  },
  failed: {
    label: "Failed",
    verdict: "Fail",
    icon: CircleXIcon,
    tone: "text-fail",
    rail: "border-l-fail",
  },
  missing: {
    label: "Missing data",
    verdict: "Missing data",
    icon: CircleDashedIcon,
    tone: "text-missing",
    rail: "border-l-missing",
  },
  error: {
    label: "Error",
    verdict: "Error",
    icon: TriangleAlertIcon,
    tone: "text-destructive",
    rail: "border-l-destructive",
  },
  pruned: {
    label: "Pruned",
    verdict: "Pruned",
    icon: CircleMinusIcon,
    tone: "text-muted-foreground",
    rail: "border-l-border",
  },
  unknown: {
    label: "Unknown",
    verdict: "Unknown",
    icon: CircleHelpIcon,
    tone: "text-muted-foreground",
    rail: "border-l-border",
  },
}
