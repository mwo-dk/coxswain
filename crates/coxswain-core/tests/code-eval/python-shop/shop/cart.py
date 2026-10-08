"""Shopping carts, kept in memory until checkout."""

from dataclasses import dataclass, field

from shop.cache import cached_price
from shop.pricing import apply_coupon


@dataclass
class Cart:
    """A customer's cart: SKUs and how many of each."""

    id: str
    items: dict[str, int] = field(default_factory=dict)

    def add_item(self, sku: str, quantity: int = 1) -> None:
        if quantity <= 0:
            raise ValueError("quantity must be positive")
        self.items[sku] = self.items.get(sku, 0) + quantity

    def remove_item(self, sku: str) -> None:
        self.items.pop(sku, None)

    def total(self, coupon: str | None = None) -> float:
        """The sum of the items' prices, less the coupon's discount, before VAT."""
        subtotal = sum(cached_price(sku) * n for sku, n in self.items.items())
        return apply_coupon(subtotal, coupon) if coupon else subtotal
