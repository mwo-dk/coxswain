"""The web application: every route of the shop's HTTP API is mounted here."""

from fastapi import FastAPI, Depends, HTTPException

from shop.cart import Cart
from shop.db import get_session, Order
from shop.pricing import price_with_vat


app = FastAPI(title="Shop")
carts: dict[str, Cart] = {}


@app.post("/carts/{cart_id}/items")
def add_item(cart_id: str, sku: str, quantity: int = 1):
    """Put `quantity` of `sku` into the cart, making the cart if it is new."""
    cart = carts.setdefault(cart_id, Cart(cart_id))
    cart.add_item(sku, quantity)
    return {"items": cart.items}


@app.get("/carts/{cart_id}/total")
def cart_total(cart_id: str, coupon: str | None = None):
    if cart_id not in carts:
        raise HTTPException(status_code=404, detail="no such cart")
    return {"total": carts[cart_id].total(coupon)}


@app.post("/carts/{cart_id}/checkout")
def checkout(cart_id: str, session=Depends(get_session)):
    """Turn the cart into an order stored in the database and forget the cart."""
    cart = carts.pop(cart_id, None)
    if cart is None:
        raise HTTPException(status_code=404, detail="no such cart")
    order = Order(cart_id=cart_id, total=price_with_vat(cart.total()))
    session.add(order)
    session.commit()
    return {"order": order.id}
