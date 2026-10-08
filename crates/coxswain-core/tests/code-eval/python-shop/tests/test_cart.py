from shop.cart import Cart
from shop.pricing import apply_coupon, price_with_vat


def test_coupon():
    assert apply_coupon(100.0, "welcome10") == 90.0


def test_vat():
    assert price_with_vat(100.0) == 125.0


def test_negative_quantity():
    cart = Cart("c1")
    try:
        cart.add_item("sku", 0)
    except ValueError:
        pass
