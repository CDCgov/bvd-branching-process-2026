from typing import Literal

from .base_class import NaturalHistoryProcessor
from .detection_band_processor import DetectionBandProcessor


def init_processor(
    strategy: Literal["detection_band"],
) -> NaturalHistoryProcessor:
    if strategy == "detection_band":
        return DetectionBandProcessor(detection_band=(0.2, 0.6))
    else:
        raise ValueError(f"Unknown processor name: {strategy}")


__all__ = [
    "NaturalHistoryProcessor",
    "DetectionBandProcessor",
    "init_processor",
]
