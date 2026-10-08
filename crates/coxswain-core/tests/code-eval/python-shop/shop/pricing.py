"""Prices: VAT and coupons."""

VAT_RATE = 0.25
COUPONS = {"WELCOME10": 0.10, "SUMMER20": 0.20}


def price_with_vat(net: float) -> float:
    """The price the customer pays: the net price plus 25 % VAT, rounded to cents."""
    return round(net * (1 + VAT_RATE), 2)


def apply_coupon(amount: float, code: str) -> float:
    """`amount` less the coupon's share; an unknown code changes nothing."""
    discount = COUPONS.get(code.upper(), 0.0)
    return round(amount * (1 - discount), 2)
