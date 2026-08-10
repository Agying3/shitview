from __future__ import annotations

from datetime import datetime, timezone


def format_relative_time(timestamp: float, now: datetime | None = None) -> str:
    current = now or datetime.now(timezone.utc)
    target = datetime.fromtimestamp(timestamp, timezone.utc)
    seconds = max(0, int((current - target).total_seconds()))

    if seconds < 60:
        return "just now"
    minutes = seconds // 60
    if minutes < 60:
        return _unit(minutes, "minute")
    hours = minutes // 60
    if hours < 24:
        return _unit(hours, "hour")
    days = hours // 24
    if days < 30:
        return _unit(days, "day")
    months = days // 30
    if months < 12:
        return _unit(months, "month")
    years = days // 365
    return _unit(years, "year")


def _unit(value: int, name: str) -> str:
    suffix = "" if value == 1 else "s"
    return f"{value} {name}{suffix} ago"
