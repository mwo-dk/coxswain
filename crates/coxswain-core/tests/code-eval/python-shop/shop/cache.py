"""Product prices cached in Redis, so the catalogue database is not asked for every cart."""

import redis

from shop.db import product_price

PRICE_TTL_SECONDS = 300
_redis = redis.Redis(host="localhost", port=6379, decode_responses=True)


def cached_price(sku: str) -> float:
    """The price of `sku`: from Redis when it was looked up in the last five minutes."""
    key = f"price:{sku}"
    hit = _redis.get(key)
    if hit is not None:
        return float(hit)
    price = product_price(sku)
    _redis.setex(key, PRICE_TTL_SECONDS, price)
    return price
