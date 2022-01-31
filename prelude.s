type Option a = | None | Some a in
type List a = | Nil | Cons a (List a) in

type Eq a = {
    (==) : a -> a -> Bool
} in

let eq_Int = {
    (==) = \l r -> l #Int== r
} in
let eq_Float = {
    (==) = \l r -> l #Float== r
} in
let eq_Option: Eq a -> Eq (Option a) = \eq_a -> {
    (==) = \l r ->
        case l of
            | Some l_val ->
                case r of
                    | Some r_val -> eq_a.(==) l_val r_val
                    | None -> False
            | None -> 
                case r of
                    | Some _ -> False
                    | None -> True
} in
let eq_List: Eq a -> Eq (List a) = \d -> {
    (==) = let f l r = case l of
                | Nil -> case r of
                    | Nil -> True
                    | Cons x y -> False
                | Cons x xs -> case r of
                    | Nil -> False
                    | Cons y ys -> d.(==) x y && f xs ys
            in f
} in

type Ordering = | LT | EQ | GT
in

type Ord a = {
    compare : a -> a -> Ordering
} in

let ord_Int = {
    compare = \l r ->
        if l #Int< r
        then LT
        else if l #Int== r
        then EQ
        else GT
} in

let ord_Float = {
    compare = \l r ->
        if l #Float< r
        then LT
        else if l #Float== r
        then EQ
        else GT
} in
let ord_Option: Ord a -> Ord (Option a) = \compare_a -> {
    compare = \l r ->
        case l of
            | Some l_val ->
                case r of
                    | Some r_val -> compare_a.compare l_val r_val
                    | None -> LT
            | None -> 
                case r of
                    | Some r_val -> GT
                    | None -> EQ
} in
type Num a = {
    (+) : a -> a -> a,
    (-) : a -> a -> a,
    (*) : a -> a -> a,
    negate: a -> a
} in
let num_Int: Num Int = {
    (+) = \l r -> l #Int+ r,
    (-) = \l r -> l #Int- r,
    (*) = \l r -> l #Int* r,
    negate = \x -> 0 #Int- x
} in
let num_Float: Num Float = {
    (+) = \l r -> l #Float+ r,
    (-) = \l r -> l #Float- r,
    (*) = \l r -> l #Float* r,
    negate = \x -> 0.0 #Float- x
} in
type Functor f = {
    map : (a -> b) -> f a -> f b
} in
let functor_Option: Functor Option = {
    map = \f x -> case x of
                    | Some y -> Some (f y)
                    | None -> None
} in
type Applicative f = {
    (<*>) : f (a -> b) -> f a -> f b,
    pure : a -> f a
} in
let applicative_Option: Applicative Option = {
    (<*>) = \f x -> case f of
                        | Some g ->
                            case x of
                                | Some y -> Some (g y)
                                | None -> None
                        | None -> None,
    pure = \x -> Some x
} in
type Monad m = {
    (>>=) : m a -> (a -> m b) -> m b,
    return : a -> m a
} in
let monad_Option: Monad Option = {
    (>>=) = \m f -> case m of
                        | Some x -> f x
                        | None -> None,
    return = \x -> Some x
} in
{ ord_Option = ord_Option, ord_Float = ord_Float, ord_Int = ord_Int, eq_List = eq_List, eq_Option = eq_Option, eq_Float = eq_Float, eq_Int = eq_Int, num_Int = num_Int, num_Float = num_Float, functor_Option = functor_Option }

