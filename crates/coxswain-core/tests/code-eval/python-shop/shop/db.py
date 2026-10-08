"""The database: SQLAlchemy models and sessions over PostgreSQL."""

import os

from sqlalchemy import create_engine, Column, Integer, String, Numeric
from sqlalchemy.orm import declarative_base, sessionmaker

DATABASE_URL = os.environ.get("SHOP_DATABASE_URL", "postgresql+psycopg://shop@localhost/shop")
engine = create_engine(DATABASE_URL, pool_size=5)
Session = sessionmaker(bind=engine)
Base = declarative_base()


class Product(Base):
    __tablename__ = "products"
    sku = Column(String, primary_key=True)
    name = Column(String, nullable=False)
    price = Column(Numeric(10, 2), nullable=False)


class Order(Base):
    __tablename__ = "orders"
    id = Column(Integer, primary_key=True)
    cart_id = Column(String, nullable=False)
    total = Column(Numeric(10, 2), nullable=False)


def get_session():
    """A session for one request, closed after it."""
    session = Session()
    try:
        yield session
    finally:
        session.close()


def product_price(sku: str) -> float:
    with Session() as s:
        return float(s.get(Product, sku).price)
